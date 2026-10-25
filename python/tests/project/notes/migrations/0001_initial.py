"""The one table the tiny project needs."""

from django.db import migrations, models


class Migration(migrations.Migration):
    initial = True

    dependencies: list[tuple[str, str]] = []

    operations = [
        migrations.CreateModel(
            name="Note",
            fields=[
                ("id", models.AutoField(primary_key=True, serialize=False)),
                ("owner_id", models.IntegerField()),
                ("body", models.TextField()),
            ],
        )
    ]
