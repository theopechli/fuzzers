import glob
import subprocess
import random
import time
import threading
from argparse import ArgumentParser


def fuzz(thread_id: int, input: bytearray, binary: str):
    assert isinstance(thread_id, int)
    assert isinstance(input, bytearray)

    tmpfn = f"tmpinput{thread_id}"
    with open(tmpfn, "wb") as fd:
        fd.write(input)

    sp = subprocess.Popen([binary, flag, tmpfn],
                          stdout=subprocess.DEVNULL,
                          stderr=subprocess.DEVNULL)
    ret = sp.wait()

    if ret != 0:
        print(f"Exited with {ret}")


parser = ArgumentParser()
parser.add_argument("-c", "--corpus", dest="corpus", required=True, type=str,
                    help="location of the inputs (corpus)")
parser.add_argument("-b", "--binary", dest="binary", required=True, type=str,
                    help="binary to fuzz (fuzz target)")
parser.add_argument("-f", "--flag", dest="flag", required=True, type=str,
                    help="flag passed to the binary (no dashes, full flag)")

args = parser.parse_args()

corpus_filenames = glob.glob(args.corpus + "/*")
binary = args.binary
flag = "--" + args.flag

corpus = set()
for filename in corpus_filenames:
    corpus.add(open(filename, "rb").read())

corpus = list(map(bytearray, corpus))

start = time.time()

cases = 0


def worker(thread_id):
    global start, corpus, cases

    while True:
        input = bytearray(random.choice(corpus))

        for _ in range(random.randint(1, 8)):
            input[random.randint(0, len(input) - 1)] = random.randint(0, 255)

        fuzz(thread_id, input, binary)

        cases += 1

        time_elapsed = time.time() - start

        fcps = float(cases) / time_elapsed

        print(f"[{time_elapsed:<10.4f}] cases {cases:10} | fcps {fcps:10.4f}")


for thread_id in range(4):
    threading.Thread(target=worker, args=[thread_id]).start()


while threading.active_count() > 1:
    time.sleep(0.1)
