#!/usr/bin/env python
"""Django's command-line utility, exactly as `startproject` writes one.

Nothing in this suite runs it: it is here because it is part of the layout the
fixture is about, and a project that has it is the one a developer has.
"""

import os
import sys


def main():
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "myshop.settings")
    from django.core.management import execute_from_command_line

    execute_from_command_line(sys.argv)


if __name__ == "__main__":
    main()
