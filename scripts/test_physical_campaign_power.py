#!/usr/bin/env python3
import json
from pathlib import Path
import tempfile
import unittest
from physical_campaign_power import AdbPower, Journal, PowerError, RestorationError, run_campaign, restore


class FakePower:
    def __init__(self, initial=True, fail_confirm=False, fail_restore=False):
        self.mode = self.original = initial
        self.fail_confirm, self.fail_restore = fail_confirm, fail_restore
        self.operations = []
        self.device_tag = 'test-device'
    def initial(self): return self.original
    def set_mode(self, enabled):
        self.operations.append(('set', enabled)); self.mode = enabled; return 10
    def wait_health(self, expected, after_ms=None):
        self.operations.append(('health', expected))
        if self.fail_confirm and not expected: raise PowerError('OEM did not confirm OFF')
        if self.mode != expected: raise PowerError('mismatch')
        return dict(power_save=expected, at_ms=12, age_ms=1)
    def restore(self, initial):
        self.operations.append(('restore', initial))
        if self.fail_restore: raise PowerError('device unplugged')
        self.mode = initial
        return dict(power_save=initial, at_ms=20, age_ms=1)


class PowerTests(unittest.TestCase):
    def run_case(self, power, runner=lambda _: 0):
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'state.json'
            code=run_campaign(power,path,['test'],runner)
            record=json.loads(path.read_text())
            self.assertEqual(path.stat().st_mode & 0o777,0o600)
            self.assertEqual(record['stage'],'RESTORED')
            self.assertTrue(record['restoration_verified'])
            return code,record

    def test_on_restored_after_success_and_nonzero_test(self):
        for code in [0,7]:
            p=FakePower(True)
            def runner(_):
                self.assertFalse(p.mode)
                self.assertEqual(p.operations[-1],('health',False))
                return code
            self.assertEqual(self.run_case(p,runner)[0],code)
            self.assertTrue(p.mode)
            self.assertEqual(p.operations,[('set',False),('health',False),('restore',True)])

    def test_initial_off_stays_off(self):
        p=FakePower(False)
        self.run_case(p)
        self.assertFalse(p.mode)
        self.assertNotIn(('set',True),p.operations)

    def test_no_command_before_health_confirmation_and_restore_on_failure(self):
        p=FakePower(True,fail_confirm=True)
        called=[]
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'state.json'
            with self.assertRaises(PowerError):run_campaign(p,path,['test'],lambda _:called.append(True))
            self.assertEqual(called,[])
            self.assertTrue(p.mode)
            self.assertEqual(json.loads(path.read_text())['stage'],'RESTORED')

    def test_interruption_restores_and_existing_journal_is_not_overwritten(self):
        p=FakePower(True)
        def interrupt(_):raise KeyboardInterrupt
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'state.json'
            with self.assertRaises(KeyboardInterrupt):run_campaign(p,path,['test'],interrupt)
            self.assertTrue(p.mode)
            saved=path.read_bytes()
            with self.assertRaises(FileExistsError):run_campaign(p,path,['test'],lambda _:0)
            self.assertEqual(path.read_bytes(),saved)

    def test_restore_failure_overrides_success_and_keeps_initial_state(self):
        p=FakePower(True,fail_restore=True)
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'state.json'
            with self.assertRaises(RestorationError):run_campaign(p,path,['test'],lambda _:0)
            record=json.loads(path.read_text())
            self.assertTrue(record['initial_power_save'])
            self.assertEqual(record['stage'],'RESTORATION_FAILED')
            p.fail_restore=False
            restore(p,Journal(path))
            self.assertTrue(p.mode)
            self.assertEqual(json.loads(path.read_text())['stage'],'RESTORED')

    def test_stale_or_unknown_health_never_passes(self):
        class Reader(AdbPower):
            text='HEALTH_SAMPLE at_ms=100 result=0 power_save=false'
            def read(self,*args):return '123' if args[0]=='shell' else self.text
            def boot_ms(self):return 6101
        p=Reader()
        with self.assertRaisesRegex(PowerError,'stale'):p.health()
        p.text='HEALTH_SAMPLE at_ms=6100 result=-1 power_save=false'
        with self.assertRaisesRegex(PowerError,'UNKNOWN'):p.health()
        p.text='HEALTH_SAMPLE at_ms=6100 result=0 power_save=false'
        self.assertFalse(p.health()['power_save'])

    def test_restore_rejects_wrong_device_and_attempts_rollback_on_journal_error(self):
        p=FakePower(True)
        with tempfile.TemporaryDirectory() as d:
            j=Journal(Path(d)/'state.json', True)
            j.update(device_tag='different-device')
            with self.assertRaises(RestorationError):restore(p,j)
            self.assertEqual(p.operations,[])
            j.data['device_tag']=p.device_tag
            def disk_full(**_):raise OSError('disk full')
            j.update=disk_full
            p.mode=False
            with self.assertRaises(OSError):restore(p,j)
            self.assertTrue(p.mode)


if __name__=='__main__':unittest.main()
