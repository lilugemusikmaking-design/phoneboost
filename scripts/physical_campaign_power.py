#!/usr/bin/env python3
"""Development-only Battery Saver guard. No PhoneBoost product API is changed.

The diagnostic app/Health service must stay running until restoration finishes.
A private journal survives SIGKILL or unplugging; use --restore JOURNAL to recover.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time


class PowerError(RuntimeError): pass
class RestorationError(PowerError): pass


def utc(): return datetime.datetime.now(datetime.timezone.utc).isoformat()


class Journal:
    def __init__(self, path, initial=None):
        self.path = Path(path)
        if initial is None:
            self.data = json.loads(self.path.read_text())
            if self.data.get('schema') != 1 or type(self.data.get('initial_power_save')) is not bool:
                raise PowerError('invalid restoration journal')
        else:
            self.path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            self.data = dict(schema=1, initial_power_save=initial, stage='CAPTURED', created_utc=utc())
            fd = os.open(self.path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, 'w') as f:
                json.dump(self.data, f); f.flush(); os.fsync(f.fileno())

    def update(self, **fields):
        self.data.update(fields, updated_utc=utc())
        fd, name = tempfile.mkstemp(prefix='.power-', dir=self.path.parent)
        try:
            with os.fdopen(fd, 'w') as f:
                json.dump(self.data, f, indent=2); f.write('\n'); f.flush(); os.fsync(f.fileno())
            os.replace(name, self.path)
        finally:
            if os.path.exists(name): os.unlink(name)


class AdbPower:
    def __init__(self, serial=None, package='org.phoneboost.app.multicore', timeout=30):
        self.adb = ['adb', '-s', serial] if serial else ['adb', '-d']
        self.package, self.timeout = package, timeout
        self.device_tag = None

    def pin_device(self):
        serial = self.read('get-serialno')
        if not serial or serial == 'unknown' or any(c.isspace() for c in serial):
            raise PowerError('ADB device identity unavailable')
        self.adb = ['adb', '-s', serial]
        self.device_tag = hashlib.sha256(serial.encode()).hexdigest()

    def read(self, *args):
        try:
            r = subprocess.run(self.adb + list(args), capture_output=True, text=True, timeout=10)
        except (OSError, subprocess.TimeoutExpired) as e:
            raise PowerError('ADB unavailable or timed out') from e
        if r.returncode != 0:
            raise PowerError('ADB operation failed: ' + ' '.join(args[:3]))
        return r.stdout.strip()

    def setting(self):
        value = self.read('shell', 'settings', 'get', 'global', 'low_power')
        if value not in ('0', '1'): raise PowerError('Battery Saver setting unavailable')
        return value == '1'

    def boot_ms(self):
        return int(float(self.read('shell', 'cat', '/proc/uptime').split()[0]) * 1000)

    def health(self):
        pid = self.read('shell', 'pidof', self.package)
        if not pid.isdigit(): raise PowerError('diagnostic Health process unavailable')
        text = self.read('logcat', '-d', '--pid=' + pid, '-t', '200', '-s', 'PhoneBoostA6:I')
        lines = [s.split('HEALTH_SAMPLE ', 1)[1] for s in text.splitlines() if 'HEALTH_SAMPLE ' in s]
        if not lines: raise PowerError('no official PhoneBoost Health sample')
        sample = dict(re.findall(r'(\w+)=([^\s]+)', lines[-1]))
        try:
            sampled = int(sample['at_ms']); age = self.boot_ms() - sampled
            bit = sample['power_save']
        except (KeyError, ValueError) as e:
            raise PowerError('invalid Health sample') from e
        if sample.get('result') != '0' or bit not in ('true', 'false') or not 0 <= age <= 6000:
            raise PowerError('Health UNKNOWN, incoherent or stale')
        return dict(power_save=bit == 'true', at_ms=sampled, age_ms=age)

    def wait_health(self, expected, after_ms=None):
        deadline = time.monotonic() + self.timeout
        last = 'no confirmation'
        while True:
            try:
                sample = self.health()
                if (sample['power_save'] == expected and self.setting() == expected
                        and (after_ms is None or sample['at_ms'] > after_ms)):
                    return sample
                last = 'Health/system do not yet confirm expected powerSave'
            except PowerError as e:
                last = str(e)
            if time.monotonic() >= deadline:
                raise PowerError('Battery Saver confirmation failed: ' + last)
            time.sleep(min(1, max(0, deadline - time.monotonic())))

    def initial(self):
        initial = self.setting()
        self.wait_health(initial)
        return initial

    def set_mode(self, enabled):
        before = self.boot_ms()
        # Command exit=0 is NOT evidence that an OEM applied the requested mode.
        self.read('shell', 'cmd', 'power', 'set-mode', '1' if enabled else '0')
        return before

    def restore(self, initial):
        # Even if a failed command applied partially or the campaign changed mode,
        # restore the recorded initial setting, then require fresh Health agreement.
        try:
            current = self.setting()
            sample = self.health()
            unchanged = current == initial and sample['power_save'] == initial
        except PowerError:
            unchanged = False
        after = None if unchanged else self.set_mode(initial)
        return self.wait_health(initial, after)


def restore(power, journal):
    # A first Ctrl-C arriving during normal cleanup must not abort rollback.
    handlers = {s: signal.getsignal(s) for s in (signal.SIGINT, signal.SIGTERM)}
    try:
        for s in handlers: signal.signal(s, signal.SIG_IGN)
        return restore_observed(power, journal)
    finally:
        for s, handler in handlers.items(): signal.signal(s, handler)


def restore_observed(power, journal):
    if journal.data.get('device_tag') != power.device_tag or not power.device_tag:
        raise RestorationError('BATTERY_SAVER_RESTORATION_FAILED: wrong/unknown device for journal')
    try:
        journal.update(stage='RESTORING')
    except OSError:
        # A journal/disk error must never prevent the actual rollback attempt.
        pass
    try:
        sample = power.restore(journal.data['initial_power_save'])
    except Exception as e:
        try:
            journal.update(stage='RESTORATION_FAILED', restoration_error=str(e))
        except OSError:
            pass
        raise RestorationError('BATTERY_SAVER_RESTORATION_FAILED; use --restore with this journal') from e
    journal.update(stage='RESTORED', restored_health=sample, restoration_verified=True)


def run_campaign(power, state_path, command, runner):
    initial = power.initial()  # Unknown/stale => no mutation and no campaign.
    journal = Journal(state_path, initial)
    try:
        journal.update(device_tag=power.device_tag)
        journal.update(stage='PREPARING')
        after = power.set_mode(False) if initial else None
        verified = power.wait_health(False, after)
        journal.update(stage='RUNNING', campaign_health=verified)
        code = runner(command)
        journal.update(command_returncode=code)
        return code
    except BaseException as e:
        journal.update(campaign_error_type=type(e).__name__, campaign_error=str(e))
        raise
    finally:
        # If confirmation/start/test fails or is interrupted, restoration still runs.
        restore(power, journal)


def child_run(command):
    child = subprocess.Popen(command, start_new_session=True)
    try:
        return child.wait()
    finally:
        if child.poll() is None:
            os.killpg(child.pid, signal.SIGTERM)
            try: child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()


def interrupted(signum, _frame):
    # Stop only our child before restoring. A second Ctrl-C must not skip cleanup.
    signal.signal(signal.SIGINT, signal.SIG_IGN)
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    raise KeyboardInterrupt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument('--state', type=Path, help='new private journal for this campaign')
    group.add_argument('--restore', type=Path, help='restore from an existing journal, no workload')
    parser.add_argument('--serial')
    parser.add_argument('--package', default='org.phoneboost.app.multicore')
    parser.add_argument('--timeout', type=float, default=30, help='bounded observation window, seconds; not a PhoneBoost timeout')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if not 1 <= args.timeout <= 120: parser.error('observation timeout must be 1..120 s')
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if args.state and not command: parser.error('a campaign command is required after --')
    if args.restore and command: parser.error('--restore cannot run a campaign')
    power = AdbPower(args.serial, args.package, args.timeout)
    signal.signal(signal.SIGINT, interrupted); signal.signal(signal.SIGTERM, interrupted)
    try:
        power.pin_device()
        if args.restore:
            restore(power, Journal(args.restore)); code = 0
        else:
            code = run_campaign(power, args.state, command, child_run)
        print('Battery Saver: initial state restored and confirmed by fresh Health.', flush=True)
        return code if code >= 0 else 128 - code
    except RestorationError as e:
        print(str(e), file=sys.stderr); return 4
    except (PowerError, OSError, ValueError) as e:
        print('Campaign refused/failed: ' + str(e), file=sys.stderr); return 2
    except KeyboardInterrupt:
        print('Campaign interrupted; inspect restoration journal.', file=sys.stderr); return 130


if __name__ == '__main__': sys.exit(main())
