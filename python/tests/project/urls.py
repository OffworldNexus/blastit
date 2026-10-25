"""The URLs of the tiny project, including the one blastit has no route for."""

from django.urls import path

import views

urlpatterns = [
    path("notes/", views.note_list),
    path("notes/create/", views.note_create),
    path("notes/<int:pk>/delete/", views.note_delete),
    path("notes/atomic/", views.note_atomic),
    path("notes/in/", views.note_in),
    path("notes/multi/", views.note_multi),
    path("notes/bulk/", views.note_bulk),
    path("notes/types/", views.note_types),
    path("oops/", views.oops),
]
