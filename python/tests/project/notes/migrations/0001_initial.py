"""The one table the tiny project needs."""

from typing import Any, ClassVar

from django.db import migrations, models


class Migration(migrations.Migration):
    initial: ClassVar[bool] = True

    dependencies: ClassVar[list[tuple[str, str]]] = []

    operations: ClassVar[list[Any]] = [
        migrations.CreateModel(
            name="Note",
            fields=[
                ("id", models.AutoField(primary_key=True, serialize=False)),
                ("owner_id", models.IntegerField()),
                ("body", models.TextField()),
            ],
        )
    ]
