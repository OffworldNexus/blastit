<?php

// `exit(3)` is a `zend_bailout()`: a `longjmp` through `EG(bailout)`. The shim
// must trap it and still report the response the script had already written.
// The non-zero argument is deliberate: the worker must never mistake it for
// the status to send, and must never let it reach its own exit code.
echo 'bye from exit';
exit(3);
