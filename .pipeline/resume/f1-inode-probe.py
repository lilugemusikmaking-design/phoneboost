import os,socket,tempfile,json,subprocess
for root in ['/tmp','/var/tmp']:
 for pin in [False,True]:
  with tempfile.TemporaryDirectory(prefix='pb-inode-',dir=root) as d:
   p=d+'/control.sock'
   s=socket.socket(socket.AF_UNIX);s.bind(p);s.close()
   first=os.lstat(p)
   fd=os.open(p,os.O_PATH|os.O_NOFOLLOW) if pin else None
   os.unlink(p)
   s=socket.socket(socket.AF_UNIX);s.bind(p);s.close()
   second=os.lstat(p)
   print(json.dumps(dict(root=root,pinned=pin,first_inode=first.st_ino,second_inode=second.st_ino,same_identity=(first.st_dev,first.st_ino,first.st_uid,first.st_mode)==(second.st_dev,second.st_ino,second.st_uid,second.st_mode))))
   if fd is not None:os.close(fd)
