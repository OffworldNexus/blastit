<?php
// A real `.php` file under the served root: the mount must answer 404 for it,
// never its source. The static leg installs the decoy tree around this file.
echo "source-disclosure\n";
