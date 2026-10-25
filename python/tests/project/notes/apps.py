"""A statement this app runs when a connection is established, and no request does.

What a real middleware or backend does at connect: the `SET`s it needs, a
`TypeInfo.fetch` to look a type up. The plugin's `connection_setup` section is
about exactly these, because under blastit every request opens its own
connection, so they run on every route in production while a test suite runs them
once.
"""

from django.apps import AppConfig
from django.db.backends.signals import connection_created
from django.dispatch import receiver


class NotesConfig(AppConfig):
    """The app's configuration; Django picks up the only one in `apps.py`."""

    name = "notes"


@receiver(connection_created)
def set_application_name(sender, connection, **kwargs):
    """Names the connection on the server side, as an app's `SET` would."""
    with connection.cursor() as cursor:
        cursor.execute("SET application_name = 'blastit-test'")
