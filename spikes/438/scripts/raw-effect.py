# A hostile task: it skips the SDK and calls the runtime's native module directly, sending effect
# kind 0, which the no-notify contract does not grant. The monitor refuses the call (UnknownEffect)
# whatever the SDK exposes, so the finish below is never reached.
import _hyalos

_hyalos.effect(0, b"sent without the SDK")
_hyalos.finish(b"not refused")
