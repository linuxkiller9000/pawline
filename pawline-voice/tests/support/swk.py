import socket,struct,glob,os,sys
p=glob.glob("/tmp/xdg/sway-ipc.*")[0]
s=socket.socket(socket.AF_UNIX); s.connect(p)
c=sys.argv[1].encode()
s.sendall(b"i3-ipc"+struct.pack("<II",len(c),0)+c)
print(s.recv(4096)[14:].decode())
