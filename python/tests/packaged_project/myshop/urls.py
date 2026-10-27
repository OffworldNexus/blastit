"""The URLs of the packaged project: one route, which the manifest grants.

The path is under no mount prefix on purpose: `PATH_INFO` is what blastit routed
on, and this project exists to check the layout, not the routing.
"""

from django.urls import path

from myshop import views

urlpatterns = [
    path("packaged/", views.packaged),
]
