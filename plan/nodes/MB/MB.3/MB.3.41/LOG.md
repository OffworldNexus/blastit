# Log MB.3.41

## 2026-11-15T02:02 [lead]

created (task) A completed 5xx must say why it failed: contained bailout vs the app's own 500, and the scheduler's 502/504 must not pay for queue wait

## 2026-11-15T03:43 [lead]

status ready -> active

## 2026-11-15T03:43 [lead]

attempt 1/3 started

## 2026-11-15T03:43 [lead]

base d1a016b

## 2026-11-15T03:51 [lead]

CI flake folded into this node (owner's session): the `test` job went red on `php::tests::a_response_body_before_the_head_is_a_protocol_error` at php.rs:1138:43 with EPIPE. Diagnosis: 1138 is the TEST PEER's `Peer::send` write_all().expect("write"), not the fixture's guarded write_head/write_body. The test sends ReqHead then ReqEnd; BodyFirst::serve writes a body with no head, the transport refuses it and the view retires, so if retirement lands between the two sends the second write gets EPIPE. Same shape as the supervise EPIPE and Toolchain::probe flakes: a fixture writes to a peer that may already be gone. Fix belongs here because php.rs is this node's path and already in its diff; attribution is recorded so the commit message can state both.

## 2026-11-15T03:58 [lead]

status active -> review

## 2026-11-15T04:06 [lead]

status review -> done
