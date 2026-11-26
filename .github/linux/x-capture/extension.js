import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const IFACE = '<node><interface name="org.scooch.Capture"><method name="Calm"><arg type="b" direction="out" name="shown"/></method><method name="Capture"><arg type="s" direction="in" name="path"/><arg type="b" direction="out" name="done"/></method></interface></node>';

export default class CaptureExtension extends Extension {
    enable() {
        this._object = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this._object.export(Gio.DBus.session, '/org/scooch/Capture');
        this._owner = Gio.bus_own_name_on_connection(Gio.DBus.session, 'org.scooch.Capture', Gio.BusNameOwnerFlags.NONE, null, null);
    }

    disable() {
        this._object?.unexport();
        this._object = null;
        if (this._owner)
            Gio.bus_unown_name(this._owner);
        this._owner = 0;
    }

    Calm() {
        Main.overview.hide();
        return Main.overview.visible;
    }

    CaptureAsync(params, invocation) {
        const [path] = params;
        try {
            const stream = Gio.File.new_for_path(path).replace(null, false, Gio.FileCreateFlags.NONE, null);
            const shooter = new Shell.Screenshot();
            shooter.screenshot(false, stream, (source, result) => {
                try {
                    source.screenshot_finish(result);
                    stream.close(null);
                    invocation.return_value(new GLib.Variant('(b)', [true]));
                } catch (e) {
                    logError(e, 'capture');
                    invocation.return_value(new GLib.Variant('(b)', [false]));
                }
            });
        } catch (e) {
            logError(e, 'capture');
            invocation.return_value(new GLib.Variant('(b)', [false]));
        }
    }
}
