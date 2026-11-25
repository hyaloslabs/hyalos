# A hand-typed stub of spikes/438/sdk/no-notify.py, the SDK generated from the contract
# {"asks":[0],"may_touch":[]}, for Monty's type checker (Slice 4 of docs/plans/hyalos-kernel-python,
# S-05.3; spikes/438/monty/typecheck.sh). It types the public `task` exactly as that SDK builds it:
# `read_inputs`, `ask.classify` and `finish`, and no `notify`, since the contract grants no effect.

class _Issue:
    number: int | None
    title: bytes | None

class _Ask:
    def classify(self, text: str | bytes) -> str: ...

class _Task:
    ask: _Ask
    def read_inputs(self) -> list[_Issue]: ...
    def finish(self, text: str | bytes) -> None: ...

task: _Task
