// Status parity with the Waybar/Polybar examples: polls the fixed command
// `pawline --ctl status` (off | hidden | idle | open | busy | confirm) and
// shows it as a coloured dot + tooltip. Click runs only `pawline --ctl toggle`.
// The widget never runs text from anywhere else; those two commands are fixed.
//
// NOT RUNTIME-TESTED: no Plasma 6 session exists in the build sandbox.
// Written against the Plasma 6 plasmoid pattern (Plasma5Support.DataSource
// executable engine + Timer polling). Please do the 30-second check in
// integrations/README.md after installing.
//
// Install by hand: kpackagetool6 --type Plasma/Applet --install integrations/kde/org.pawline.widget
import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.plasma5support as P5Support
import org.kde.kirigami as Kirigami

PlasmoidItem {
    id: root
    preferredRepresentation: compactRepresentation

    // One of: off | hidden | idle | open | busy | confirm (anything else -> off).
    property string pawState: "off"

    function label(s) {
        switch (s) {
        case "busy": return "working";
        case "confirm": return "needs your confirmation";
        case "open": return "open";
        case "hidden": return "hidden";
        case "idle": return "idle";
        default: return "off";
        }
    }
    function dotColor(s) {
        switch (s) {
        case "busy": return "#22d3ee";
        case "confirm": return "#facc15";
        case "open": return "#4ade80";
        case "hidden": return "#64748b";
        case "idle": return "#94a3b8";
        default: return "#475569";
        }
    }

    Plasmoid.toolTipMainText: "Pawline"
    Plasmoid.toolTipSubText: "Pawline: " + label(pawState) + " (click to open or close)"

    P5Support.DataSource {
        id: ctl
        engine: "executable"
        connectedSources: []
        onNewData: (source, data) => {
            disconnectSource(source)
            if (source.indexOf("--ctl status") !== -1) {
                const s = (data["stdout"] || "").trim()
                root.pawState = ["off", "hidden", "idle", "open", "busy", "confirm"].indexOf(s) !== -1 ? s : "off"
            }
        }
        function poll() { connectSource("pawline --ctl status") }
        function toggle() { connectSource("pawline --ctl toggle") }
    }

    // 5 s like the Waybar example; cheap: one word over a local socket.
    Timer {
        interval: 5000
        repeat: true
        running: true
        triggeredOnStart: true
        onTriggered: ctl.poll()
    }

    compactRepresentation: MouseArea {
        id: area
        onClicked: ctl.toggle()
        implicitWidth: Kirigami.Units.iconSizes.smallMedium
        implicitHeight: Kirigami.Units.iconSizes.smallMedium

        Kirigami.Icon {
            anchors.fill: parent
            source: "utilities-terminal"
            opacity: root.pawState === "off" ? 0.5 : 1.0
        }
        Rectangle {
            width: 8
            height: 8
            radius: 4
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            color: root.dotColor(root.pawState)
            border.color: "#00000066"
            border.width: 1
        }
    }
}
