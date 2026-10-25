"""The URLs of the tiny project, including the one blastit has no route for."""

from django.urls import path

import views

urlpatterns = [
    path("notes/", views.note_list),
    path("notes/create/", views.note_create),
    path("notes/<int:pk>/delete/", views.note_delete),
    path("notes/atomic/", views.note_atomic),
    path("oops/", views.oops),
]
