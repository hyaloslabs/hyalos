"""A stand-in for a runtime guest's native `_hyalos` module, for `check.sh` only.

    python3 -I fake_hyalos.py <sdk.py> <script.py> <data file> [label]

Runs the script under the host's Python as a runtime guest's `run` would (design.md §2, Runtime
guest `run`): the SDK executed as module `hyalos`, then the script as the main program. Records 0
to 2 are placeholders, the data file's non-empty lines follow from index 3, and every question is
answered with `label` (3, other, by default). It prints the effects and the result as JSON.
"""

import json
import sys
import types


class Finished(Exception):
    pass


def main():
    sdk_path, script_path, data_path = sys.argv[1:4]
    label = int(sys.argv[4]) if len(sys.argv) > 4 else 3
    with open(data_path, "rb") as f:
        data = [line for line in f.read().split(b"\n") if line]
    records = [b"contract", b"sdk", b"script"] + data
    seen = {"questions": [], "effects": [], "result": None}

    native = types.ModuleType("_hyalos")

    def read(index):
        return records[index] if 0 <= index < len(records) else None

    def infer(schema, question):
        seen["questions"].append([schema, question.decode("latin-1")])
        return label

    def effect(kind, body):
        seen["effects"].append([kind, body.decode("latin-1")])

    def finish(result):
        seen["result"] = result.decode("latin-1")
        raise Finished()

    native.read, native.infer, native.effect, native.finish = read, infer, effect, finish
    sys.modules["_hyalos"] = native

    hyalos = types.ModuleType("hyalos")
    with open(sdk_path, "rb") as f:
        exec(compile(f.read(), "hyalos", "exec"), hyalos.__dict__)
    sys.modules["hyalos"] = hyalos

    with open(script_path, "rb") as f:
        script = compile(f.read(), "script", "exec")
    try:
        exec(script, {"__name__": "__main__"})
    except Finished:
        pass
    except Exception as error:
        # As a runtime guest does: an uncaught exception ends the run with its type's name.
        seen["result"] = "error " + type(error).__name__
    print(json.dumps(seen, ensure_ascii=True))


if __name__ == "__main__":
    main()
