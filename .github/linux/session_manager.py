import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

NODE = """
<node>
  <interface name="org.gnome.SessionManager">
    <method name="IsSessionRunning">
      <arg type="b" direction="out"/>
    </method>
    <signal name="SessionRunning"/>
  </interface>
</node>
"""


def call(connection, sender, path, interface, method, parameters, invocation):
    if method == "IsSessionRunning":
        invocation.return_value(GLib.Variant("(b)", (True,)))
    else:
        invocation.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", method)


def acquired(connection, name):
    info = Gio.DBusNodeInfo.new_for_xml(NODE)
    connection.register_object("/org/gnome/SessionManager", info.interfaces[0], call, None, None)


Gio.bus_own_name(Gio.BusType.SESSION, "org.gnome.SessionManager", Gio.BusNameOwnerFlags.NONE, acquired, None, None)
GLib.MainLoop().run()
