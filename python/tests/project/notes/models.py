"""One model, so a request has something to select, insert, update and delete."""

from django.db import models


class Note(models.Model):
    """A note, owned by an integer id: no relations, so the SQL stays obvious."""

    owner_id = models.IntegerField()
    body = models.TextField()

    class Meta:
        app_label = "notes"
