import os
import sys
from subprocess import run

from linetimer import CodeTimer

INCLUDE_IO = bool(os.environ.get("INCLUDE_IO", False))
SHOW_RESULTS = bool(os.environ.get("SHOW_RESULTS", False))
LOG_TIMINGS = bool(os.environ.get("LOG_TIMINGS", False))
SCALE_FACTOR = os.environ.get("SCALE_FACTOR", "1")
PARTITION = os.environ.get("PARTITION", "1")
WRITE_PLOT = bool(os.environ.get("WRITE_PLOT", False))
FILE_TYPE = os.environ.get("FILE_TYPE", "parquet")
DATA_BASE_DIR = os.environ.get("DATA_BASE_DIR", "../../../")
TEST_RESULTS = os.environ.get("TEST_RESULTS", False)
print("include io:", INCLUDE_IO)
print("show results:", SHOW_RESULTS)
print("log timings:", LOG_TIMINGS)
print("file type:", FILE_TYPE)

CWD = os.path.dirname(os.path.realpath(__file__))
DATASET_BASE_DIR = os.path.join(DATA_BASE_DIR, f"resources/tpc-h/data/scale={SCALE_FACTOR}/partition={PARTITION}/parquet/")
ANSWERS_BASE_DIR = os.path.join(DATA_BASE_DIR, f"resources/tpc-h/data/scale={SCALE_FACTOR}/original/")
OUTPUT_BASE_DIR = os.path.join(CWD, f"outputs/scale={SCALE_FACTOR}/partition={PARTITION}")
if not os.path.exists(OUTPUT_BASE_DIR):
    os.makedirs(OUTPUT_BASE_DIR, exist_ok=True)
TIMINGS_FILE = os.path.join(CWD, f"{OUTPUT_BASE_DIR}/timings.csv")
DEFAULT_PLOTS_DIR = os.path.join(CWD, f"{OUTPUT_BASE_DIR}/plots")


def append_row(solution: str, q: str, secs: float, version: str, success=True):
    with open(TIMINGS_FILE, "a") as f:
        if f.tell() == 0:
            f.write("solution,version,query_no,duration[s],include_io,success\n")
        f.write(f"{solution},{version},{q},{secs},{INCLUDE_IO},{success}\n")


def on_second_call(func):
    def helper(*args, **kwargs):
        helper.calls += 1

        # first call is outside the function
        # this call must set the result
        if helper.calls == 1:
            # include IO will compute the result on the 2nd call
            if not INCLUDE_IO:
                helper.result = func(*args, **kwargs)
            return helper.result

        # second call is in the query, now we set the result
        if INCLUDE_IO and helper.calls == 2:
            helper.result = func(*args, **kwargs)

        return helper.result

    helper.calls = 0
    helper.result = None

    return helper


def execute_all(solution: str):
    package_name = f"{solution}_queries"
    num_queries = 22

    with CodeTimer(name=f"Overall execution of ALL {solution} queries", unit="s"):
        for i in range(1, num_queries + 1):
            run([sys.executable, "-m", f"{package_name}.q{i}"])
