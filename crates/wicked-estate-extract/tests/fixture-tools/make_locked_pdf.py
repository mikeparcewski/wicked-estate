"""Writes locked.pdf: a one-page PDF under the Standard security handler (V1, R2, RC4-40) whose
user password is not empty, so a reader with no password cannot open it. Made-up passwords; the
file holds no text. Run: python3 make_locked_pdf.py ../fixtures/docs/locked.pdf
(this script stays out of tests/fixtures, whose files the language fixture harness extracts)."""
import hashlib, struct, sys
PAD = bytes.fromhex("28BF4E5E4E758A4164004E56FFFA01082E2E00B6D0683E802F0CA9FE6453697A")
def rc4(key, data):
    S=list(range(256)); j=0
    for i in range(256):
        j=(j+S[i]+key[i%len(key)])%256; S[i],S[j]=S[j],S[i]
    i=j=0; out=bytearray()
    for b in data:
        i=(i+1)%256; j=(j+S[i])%256; S[i],S[j]=S[j],S[i]; out.append(b^S[(S[i]+S[j])%256])
    return bytes(out)
def pad(pw): return (pw+PAD)[:32]
user, owner = b"user-pw-not-empty", b"owner-pw"
O = rc4(hashlib.md5(pad(owner)).digest()[:5], pad(user))
P = -44
ID = bytes(range(16))
key = hashlib.md5(pad(user)+O+struct.pack("<i",P)+ID).digest()[:5]
U = rc4(key, PAD)
objs = ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>",
        "<< /Filter /Standard /V 1 /R 2 /O <%s> /U <%s> /P %d >>" % (O.hex(), U.hex(), P)]
pdf = bytearray(b"%PDF-1.4\n"); offs=[]
for i,o in enumerate(objs):
    offs.append(len(pdf)); pdf += ("%d 0 obj\n%s\nendobj\n" % (i+1,o)).encode()
x=len(pdf); pdf += b"xref\n0 5\n0000000000 65535 f \n" + b"".join(b"%010d 00000 n \n"%o for o in offs)
pdf += ("trailer\n<< /Size 5 /Root 1 0 R /Encrypt 4 0 R /ID [<%s> <%s>] >>\nstartxref\n%d\n%%%%EOF\n" % (ID.hex(), ID.hex(), x)).encode()
open(sys.argv[1],"wb").write(pdf)
