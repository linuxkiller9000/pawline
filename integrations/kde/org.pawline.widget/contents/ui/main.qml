// UNTESTED: written from the Plasma 6 plasmoid pattern, no Plasma session was available.
// Install by hand: kpackagetool6 --type Plasma/Applet --install integrations/kde/org.pawline.widget
// Click runs only the fixed command `pawline --ctl toggle`.
import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.plasma5support as P5Support
import org.kde.kirigami as Kirigami

PlasmoidItem {
    id: root
    preferredRepresentation: compactRepresentation

    P5Support.DataSource {
        id: runner
        engine: "executable"
        connectedSources: []
        onNewData: (source, data) => disconnectSource(source)
        function toggle() { connectSource("pawline --ctl toggle") }
    }

    compactRepresentation: MouseArea {
        onClicked: runner.toggle()
        Kirigami.Icon { anchors.fill: parent; source: "utilities-terminal" }
    }
}
