"""The URLs of the live project: one route blastit grants, one it does not.

`/live/oops/` exists here too, and blastit has no route for it: a request to it
never reaches Django (blastit answers 404 itself), which is what the report's
`unrouted` section is for.
"""

from django.urls import path

import views

urlpatterns = [
    path("live/", views.granted),
    path("live/denied/", views.denied),
    path("live/oops/", views.granted),
]
