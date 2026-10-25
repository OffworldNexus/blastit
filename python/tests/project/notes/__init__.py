"""The `notes` app: one model, for the requests the plugin tests make.

`apps.py` is where the connection-setup statement lives, because Django runs
`connection_created` receivers inside `DatabaseWrapper.connect()`.
"""
