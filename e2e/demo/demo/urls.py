from django.urls import path

from notes import views

urlpatterns = [
    path("", views.home),
    path("login/", views.login_view),
    path("logout/", views.logout_view),
    path("notes/", views.notes),
    path("notes/<int:note_id>/delete/", views.delete_note),
    path("notes/peek/", views.peek),
    path("notes/sql/", views.raw_sql),
    path("debug/sql/", views.raw_sql),
    path("debug/env/", views.env),
    path("debug/caps/", views.caps_probe),
    path("debug/egress/", views.egress_probe),
    path("egress/probe/", views.egress_probe),
]
