"""Create the demo users. Run with the real credentials, before blastit starts."""

import os

from django.contrib.auth.models import User
from django.core.management.base import BaseCommand


class Command(BaseCommand):
    def handle(self, *args, **options):
        password = os.environ.get("DEMO_PASSWORD", "demo-password")
        for name in ("alice", "bob"):
            user, _ = User.objects.get_or_create(username=name)
            user.set_password(password)
            user.save()
        self.stdout.write("seeded alice, bob")
