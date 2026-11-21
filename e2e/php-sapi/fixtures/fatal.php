<?php

// An uncaught fatal error is also a `zend_bailout()`, from a different place in
// the engine than `exit()`. The shim must report status 500 for a response that
// never started, and the *next* request must still be served.
undefined_probe_function();
