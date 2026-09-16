import importlib.machinery
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SCRIPT=Path(__file__).resolve().parents[2]/'scripts/project-pipeline'
loader=importlib.machinery.SourceFileLoader('pipeline',str(SCRIPT))
spec=importlib.util.spec_from_loader(loader.name,loader)
p=importlib.util.module_from_spec(spec)
loader.exec_module(p)

class PipelineSafety(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.root=Path(self.tmp.name)
        self.meta=self.root/'.pipeline'; self.meta.mkdir()
        self.patch=patch.multiple(p,ROOT=self.root,META=self.meta);self.patch.start()
        self.git('init','-b','chore/test')
        self.git('config','user.email','pipeline@example.invalid')
        self.git('config','user.name','Pipeline Test')
        p.save('STATE.json',{'branch':'chore/test'})
        p.save('AUTHORITY.json',{'order':[]})
        self.git('add','.');self.git('commit','-m','initial')
    def tearDown(self):
        self.patch.stop();self.tmp.cleanup()
    def git(self,*args):
        return subprocess.run(['git',*args],cwd=self.root,check=True,capture_output=True)
    def test_protected_branch_refused(self):
        self.git('switch','-c','master')
        with self.assertRaises(RuntimeError):p.branch_guard()
    def test_detached_refused(self):
        self.git('checkout','--detach')
        with self.assertRaises(RuntimeError):p.branch_guard()
    def test_wrong_writer_branch_refused(self):
        self.git('switch','-c','other')
        with self.assertRaises(RuntimeError):p.branch_guard()
    def test_canonical_change_is_reported_not_accepted(self):
        (self.root/'spec.txt').write_text('new')
        p.save('AUTHORITY.json',{'order':[{'path':'spec.txt','sha256':'old'}]})
        self.assertTrue(p.authority()['order'][0]['changed'])
        self.assertEqual(p.read('AUTHORITY.json')['order'][0]['sha256'],'old')
    def test_external_is_nonblocking(self):
        p.save('AUTHORITY.json',{'order':[{'path':None,'status':'external/not-local'}]})
        self.assertFalse(p.authority()['order'][0]['changed'])
    def test_atomic_json(self):
        p.save('resume/test.json',{'next':'checkpoint'})
        self.assertEqual(p.read('resume/test.json')['next'],'checkpoint')
        self.assertEqual(list((self.meta/'resume').glob('.pending-*')),[])
    def test_missing_scanner_fails_closed(self):
        with patch.object(p.shutil,'which',return_value=None):
            with self.assertRaises(RuntimeError):p.scan_index()
    def test_existing_index_is_preserved(self):
        (self.root/'work').write_text('work')
        self.git('add','work')
        from argparse import Namespace
        with self.assertRaisesRegex(RuntimeError,'Index must be empty'):
            p.checkpoint(Namespace(paths=['work']))
        self.assertEqual(self.git('diff','--cached','--name-only').stdout.strip(),b'work')
    def test_resume_does_not_mutate(self):
        before=self.git('status','--porcelain').stdout
        p.status()
        self.assertEqual(before,self.git('status','--porcelain').stdout)

    def checkpoint_args(self):
        from argparse import Namespace
        state=p.read('STATE.json');state['completed']=[]
        p.save('STATE.json',state)
        return Namespace(paths=['.pipeline'],message='test checkpoint',next='resume',writer='test')
    def test_push_failure_preserves_local_checkpoint(self):
        args=self.checkpoint_args()
        before=self.git('rev-parse','HEAD').stdout
        with patch.object(p,'scan_index'):
            with self.assertRaises(subprocess.CalledProcessError):p.checkpoint(args)
        self.assertNotEqual(before,self.git('rev-parse','HEAD').stdout)
        self.assertEqual(self.git('status','--porcelain').stdout,b'')
        self.assertEqual(p.read('resume/manifest.json')['next_action'],'resume')
    def test_real_checkpoint_push_resume(self):
        args=self.checkpoint_args()
        with tempfile.TemporaryDirectory() as remote:
            subprocess.run(['git','init','--bare',remote],check=True,capture_output=True)
            self.git('remote','add','origin',remote)
            with patch.object(p,'scan_index'):p.checkpoint(args)
            state=p.status()
            remote_head=self.git('ls-remote','origin','refs/heads/chore/test').stdout.split()[0].decode()
            self.assertEqual(state['live_head'],remote_head)
            self.assertEqual(state['checkpoint_containing_state'],remote_head)
    @unittest.skipUnless(p.shutil.which('gitleaks'),'Gitleaks integration requires installed scanner')
    def test_staged_secret_rejected_even_if_worktree_cleaned(self):
        secret=self.root/'synthetic-test-key.txt'
        secret.write_text('-----BEGIN '+'RSA PRIVATE KEY-----\n'+'A'*80+'\n-----END '+'RSA PRIVATE KEY-----\n')
        self.git('add',secret.name)
        secret.write_text('clean worktree version')
        with self.assertRaises(subprocess.CalledProcessError):p.scan_index()

if __name__=='__main__':unittest.main()
