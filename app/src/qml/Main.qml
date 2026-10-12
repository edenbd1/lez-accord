import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

// Accord. The view holds no chain state of its own: it asks the backend,
// which reads the LEZ chain and decodes approval packages from their bytes.
// It holds no keys and signs nothing; every next step is the exact `accord`
// command for the member whose turn it is.
Item {
    id: root
    readonly property var backend: logos.module("accord")

    readonly property color bg: "#101114"
    readonly property color panel: "#17191e"
    readonly property color raised: "#20242b"
    readonly property color line: "#2a2f37"
    readonly property color fg: "#e7e9ee"
    readonly property color dim: "#8b93a1"
    readonly property color accent: "#f5925e"
    readonly property color accentStrong: "#e2552b"
    readonly property color ok: "#4cc38a"
    readonly property color warn: "#f2c14e"
    readonly property color bad: "#e5484d"

    property var ms: ({})
    property var packages: []
    property string error: ""
    property bool narrow: width < 900

    function abbrev(k) { return k ? k.slice(0, 8) + "…" + k.slice(-4) : "" }
    function memberOf(key) {
        const m = ms.members || []
        for (let i = 0; i < m.length; ++i) if (m[i].key === key) return m[i]
        return null
    }
    function label(key) {
        const names = backend && backend.names ? backend.names : ({})
        return names[key] ? names[key] : abbrev(key)
    }
    function dur(ms_) {
        if (ms_ <= 0) return "0"
        const s = Math.round(ms_ / 1000)
        if (s < 120) return s + " s"
        const m = Math.round(s / 60)
        if (m < 120) return m + " min"
        return Math.round(m / 60) + " h"
    }
    function weightOf(p) {
        let w = 0
        for (let i = 0; i < p.approvals.length; ++i) {
            const m = memberOf(p.approvals[i].key)
            if (m && (m.roles_bits & 2)) w += m.weight
        }
        return w
    }
    function nextStep(p) {
        const q = p.chain || {}
        const f = p.file
        if (!p.digest_matches) return { who: "nobody", cmd: "Do not sign: the package's digest does not match its bytes." }
        if (q.status === "executed") return { who: "", cmd: "Executed at " + q.executed_at + "." }
        if (q.status === "cancelled") return { who: "", cmd: "Vetoed by the quorum." }
        if (q.status === "queued") {
            const at = q.executable_at
            if (p.decoded.kind !== "veto a queued action" && q.clock < at)
                return { who: "an executor", cmd: "Time lock: executable in " + dur(at - q.clock) + "\naccord execute " + f + " --key <your key>" }
            return { who: "an executor", cmd: "accord execute " + f + " --key <your key>" }
        }
        if (weightOf(p) >= (ms.threshold || 1))
            return { who: "anyone", cmd: "accord queue " + f }
        const missing = (ms.members || []).filter(m => (m.roles_bits & 2) && !p.approvals.some(a => a.key === m.key))
        return { who: missing.map(m => label(m.key)).join(", "), cmd: "accord decode " + f + "\naccord approve " + f + " --key <your key>" }
    }

    // Slots answer through `answered(token, json)`; each request names its own token.
    property bool ready: false
    property int nextToken: 1
    property var waiting: ({})
    function ask(slotCall, handler) {
        const token = "t" + (nextToken++)
        const w = waiting; w[token] = handler; waiting = w
        logos.watch(slotCall(token), function (r) {
            if (r && String(r).indexOf("error: ") === 0) { forget(token); handler({ ok: false, error: String(r).substring(7) }) }
        }, function (err) { forget(token); handler({ ok: false, error: "The Accord backend did not answer: " + err }) })
    }
    function forget(token) { const w = waiting; delete w[token]; waiting = w }
    function openMultisig(id) {
        id = String(id).trim(); if (id === "" || !ready) return
        root.error = ""; root.packages = []
        ask(t => backend.loadMultisig(t, id), o => {
            if (!o.ok) { root.error = o.error; return }
            root.ms = o
            msField.text = o.id_text
            if (backend.initialPackages) for (const f of backend.initialPackages) openPackage(f)
        })
    }
    function openPackage(src) {
        src = String(src).trim(); if (src === "" || !ready || !ms.ok) return
        const idx = nextToken
        ask(t => backend.loadPackage(t, ms.id_text, src), o => {
            if (!o.ok) { root.error = o.error; return }
            o.index = idx
            const list = root.packages.filter(x => !(x.digest === o.digest && x.action === o.action))
            list.push(o); list.sort((x, y) => x.index - y.index); root.packages = list
        })
    }
    Connections {
        target: logos
        ignoreUnknownSignals: true
        function onViewModuleReadyChanged(name, isReady) { if (name === "accord") root.ready = isReady && root.backend !== null }
    }
    Connections {
        target: root.backend
        ignoreUnknownSignals: true
        function onAnswered(token, json) {
            const h = root.waiting[token]
            if (!h) return
            root.forget(token)
            let o; try { o = JSON.parse(json) } catch (e) { o = { ok: false, error: "unreadable answer" } }
            h(o)
        }
    }
    Component.onCompleted: ready = backend !== null && logos.isViewModuleReady("accord")
    onReadyChanged: if (ready && backend.initialMultisig) openMultisig(backend.initialMultisig)

    Rectangle { anchors.fill: parent; color: bg }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // header
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 56
            color: panel
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 20
                anchors.rightMargin: 20
                spacing: 12
                Rectangle { width: 26; height: 26; radius: 13; color: accentStrong
                    Text { anchors.centerIn: parent; text: "A"; color: "white"; font.bold: true; font.pixelSize: 14 } }
                Text { text: "Accord"; color: fg; font.pixelSize: 18; font.bold: true }
                Text { text: ms.id_text ? "· " + ms.id_text : ""; color: dim; font.pixelSize: 15 }
                Item { Layout.fillWidth: true }
                Rectangle {
                    radius: 10; color: raised; implicitWidth: netText.implicitWidth + 24; implicitHeight: 24
                    Text { id: netText; anchors.centerIn: parent; color: dim; font.pixelSize: 12
                        text: (backend ? backend.rpc : "") + (ms.clock ? "  ·  clock " + new Date(ms.clock).toISOString().slice(11, 19) + " UTC" : "") }
                }
            }
        }
        Rectangle { Layout.fillWidth: true; height: 1; color: line }

        // inputs
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 52
            color: bg
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 20
                anchors.rightMargin: 20
                spacing: 10
                component Field: TextField {
                    color: fg; font.pixelSize: 13; placeholderTextColor: dim; selectByMouse: true
                    background: Rectangle { radius: 8; color: panel; border.color: parent.activeFocus ? accent : line }
                }
                Field { id: msField; Layout.preferredWidth: 220; placeholderText: "multisig id"; onAccepted: openMultisig(text) }
                Button { text: "Open"; onClicked: openMultisig(msField.text) }
                Field { id: pkgField; Layout.fillWidth: true; placeholderText: "approval package: a file path, or paste its JSON"; onAccepted: { openPackage(text); text = "" } }
                Button { text: "Decode"; enabled: !!ms.ok; onClicked: { openPackage(pkgField.text); pkgField.text = "" } }
                Field { id: progField; Layout.preferredWidth: 200; placeholderText: "program header (base58)"; text: backend ? backend.program : ""
                        onAccepted: logos.watch(backend.saveSettings(backend.rpc, text), function (r) { if (r) root.error = r }, function (e) {}) }
            }
        }
        Rectangle { Layout.fillWidth: true; height: 1; color: line }

        GridLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            columns: narrow ? 1 : 2
            columnSpacing: 0
            rowSpacing: 0

            // configuration pane
            Rectangle {
                Layout.fillHeight: !narrow
                Layout.fillWidth: narrow
                Layout.preferredWidth: narrow ? -1 : 380
                Layout.preferredHeight: narrow ? cfgCol.implicitHeight + 32 : -1
                color: panel
                ColumnLayout {
                    id: cfgCol
                    anchors.fill: parent
                    anchors.margins: 18
                    spacing: 10
                    Text { text: "CONFIGURATION"; color: accent; font.pixelSize: 11; font.bold: true; font.letterSpacing: 1 }
                    Text { visible: root.error !== ""; text: root.error; color: bad; wrapMode: Text.Wrap; Layout.fillWidth: true }
                    RowLayout {
                        spacing: 8
                        Text { text: (ms.threshold || "–") + " of " + (ms.members ? ms.members.filter(m => m.roles_bits & 2).length : "–"); color: fg; font.pixelSize: 26; font.bold: true }
                        Text { text: "approvals\nweight"; color: dim; font.pixelSize: 11 }
                        Item { Layout.fillWidth: true }
                        Rectangle { radius: 9; color: ms.initialised ? "#1f3a2c" : "#3a2f1a"; implicitWidth: st.implicitWidth + 16; implicitHeight: 20
                            Text { id: st; anchors.centerIn: parent; text: ms.initialised ? "active" : "not activated"; color: ms.initialised ? ok : warn; font.pixelSize: 11 } }
                    }
                    GridLayout {
                        columns: 2; columnSpacing: 12; rowSpacing: 4
                        Text { text: "epoch"; color: dim; font.pixelSize: 12 }  Text { text: ms.epoch !== undefined ? ms.epoch : "–"; color: fg; font.pixelSize: 12 }
                        Text { text: "time lock"; color: dim; font.pixelSize: 12 } Text { text: ms.time_lock_ms !== undefined ? dur(ms.time_lock_ms) : "–"; color: fg; font.pixelSize: 12 }
                        Text { text: "vault"; color: dim; font.pixelSize: 12 }  Text { text: abbrev(ms.vault) + "  ·  " + (ms.vault_native || "0") + " LGO"; color: fg; font.pixelSize: 12 }
                        Text { text: "queued / executed"; color: dim; font.pixelSize: 12 } Text { text: (ms.queued || 0) + " / " + (ms.executed || 0); color: fg; font.pixelSize: 12 }
                    }
                    Rectangle { Layout.fillWidth: true; height: 1; color: line }
                    Text { text: "MEMBERS"; color: accent; font.pixelSize: 11; font.bold: true; font.letterSpacing: 1 }
                    Repeater {
                        model: ms.members || []
                        delegate: RowLayout {
                            Layout.fillWidth: true
                            spacing: 6
                            Text { text: label(modelData.key); color: fg; font.pixelSize: 13; Layout.preferredWidth: 120; elide: Text.ElideRight }
                            Repeater {
                                model: modelData.roles
                                delegate: Rectangle { radius: 8; color: raised; implicitWidth: rl.implicitWidth + 12; implicitHeight: 18
                                    Text { id: rl; anchors.centerIn: parent; text: modelData; color: dim; font.pixelSize: 10 } }
                            }
                            Item { Layout.fillWidth: true }
                            Text { text: "w " + modelData.weight; color: dim; font.pixelSize: 11 }
                        }
                    }
                    Rectangle { Layout.fillWidth: true; height: 1; color: line; visible: (ms.limits || []).length > 0 }
                    Text { visible: (ms.limits || []).length > 0; text: "SPENDING LIMITS"; color: accent; font.pixelSize: 11; font.bold: true; font.letterSpacing: 1 }
                    Repeater {
                        model: ms.limits || []
                        delegate: Text {
                            Layout.fillWidth: true; wrapMode: Text.Wrap; color: fg; font.pixelSize: 12
                            text: modelData.amount + " " + modelData.asset.kind + " per " + dur(modelData.period_ms) + ", " +
                                  modelData.min_signers + " of " + modelData.spenders.map(k => label(k)).join(", ") +
                                  "  ·  spent " + modelData.spent
                        }
                    }
                    Item { Layout.fillHeight: true }
                    Text { text: "No keys. Signs nothing. Every action below is decoded from the exact bytes its approvals sign."; color: dim; font.pixelSize: 11; wrapMode: Text.Wrap; Layout.fillWidth: true }
                }
            }

            // actions pane
            ScrollView {
                id: scroll
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: availableWidth
                ColumnLayout {
                    width: scroll.availableWidth
                    spacing: 14
                    Item { height: 4 }
                    Text { Layout.leftMargin: 20; text: "ACTIONS"; color: accent; font.pixelSize: 11; font.bold: true; font.letterSpacing: 1 }
                    Text { Layout.leftMargin: 20; visible: packages.length === 0; text: "Open an approval package (from the room or a file)."; color: dim }
                    Repeater {
                        model: packages
                        delegate: Rectangle {
                            id: card
                            readonly property var p: modelData
                            readonly property var step: nextStep(p)
                            Layout.fillWidth: true
                            Layout.leftMargin: 20
                            Layout.rightMargin: 20
                            radius: 10
                            color: panel
                            border.color: line
                            implicitHeight: cardCol.implicitHeight + 28
                            ColumnLayout {
                                id: cardCol
                                anchors.left: parent.left; anchors.right: parent.right; anchors.top: parent.top
                                anchors.margins: 14
                                spacing: 8
                                RowLayout {
                                    Layout.fillWidth: true
                                    Text { text: p.decoded ? p.decoded.kind : "?"; color: fg; font.pixelSize: 15; font.bold: true }
                                    Text { text: "· " + abbrev(p.digest); color: dim; font.pixelSize: 12 }
                                    Item { Layout.fillWidth: true }
                                    Rectangle {
                                        radius: 9; implicitHeight: 20; implicitWidth: stt.implicitWidth + 16
                                        color: !p.digest_matches ? "#3a1c1e" : p.chain.status === "executed" ? "#1f3a2c" : p.chain.status === "queued" ? "#3a2f1a" : raised
                                        Text { id: stt; anchors.centerIn: parent; font.pixelSize: 11
                                            color: !p.digest_matches ? bad : p.chain.status === "executed" ? ok : p.chain.status === "queued" ? warn : dim
                                            text: !p.digest_matches ? "digest mismatch" : (p.chain.status || "?") }
                                    }
                                }
                                Repeater {
                                    model: p.decoded && p.decoded.calls ? p.decoded.calls : []
                                    delegate: Text {
                                        Layout.fillWidth: true; wrapMode: Text.Wrap; font.pixelSize: 13
                                        color: modelData.decoded ? fg : warn
                                        text: modelData.decoded
                                              ? modelData.decoded.instruction + " " + modelData.decoded.amount + " (" + modelData.decoded.program + ")  " + abbrev(modelData.decoded.from) + " → " + abbrev(modelData.decoded.to)
                                              : modelData.warning + "  " + modelData.program + "  data " + modelData.data_hex
                                    }
                                }
                                Text { visible: p.decoded && p.decoded.kind === "configuration change"; color: fg; font.pixelSize: 13; wrapMode: Text.Wrap; Layout.fillWidth: true
                                    text: p.decoded && p.decoded.config ? "new members: " + p.decoded.config.members.map(m => label(m.key) + " [" + m.roles.join("/") + "]").join(", ") + "; threshold " + p.decoded.config.threshold + "; time lock " + dur(p.decoded.config.time_lock_ms) : "" }
                                Text { visible: p.description !== ""; text: "Proposer's note (not signed): " + p.description; color: dim; font.italic: true; font.pixelSize: 12; wrapMode: Text.Wrap; Layout.fillWidth: true }
                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8
                                    Text { text: "approvals " + weightOf(p) + " / " + (ms.threshold || "?"); color: dim; font.pixelSize: 12 }
                                    Rectangle { Layout.fillWidth: true; height: 6; radius: 3; color: raised
                                        Rectangle { height: parent.height; radius: 3; color: weightOf(p) >= (ms.threshold || 1) ? ok : accentStrong
                                            width: parent.width * Math.min(1, weightOf(p) / Math.max(1, ms.threshold || 1)) } }
                                }
                                Flow {
                                    Layout.fillWidth: true; spacing: 6
                                    Repeater {
                                        model: (ms.members || []).filter(m => m.roles_bits & 2)
                                        delegate: Rectangle {
                                            readonly property bool signed: p.approvals.some(a => a.key === modelData.key)
                                            radius: 9; implicitHeight: 22; implicitWidth: ap.implicitWidth + 16
                                            color: signed ? "#1f3a2c" : raised
                                            Text { id: ap; anchors.centerIn: parent; font.pixelSize: 11; color: signed ? ok : dim; text: (signed ? "✓ " : "· ") + label(modelData.key) }
                                        }
                                    }
                                }
                                Text { color: dim; font.pixelSize: 12; text: "expires " + new Date(p.expires_at).toISOString().replace("T", " ").slice(0, 16) + " UTC · epoch " + p.epoch }
                                Rectangle {
                                    Layout.fillWidth: true
                                    radius: 8
                                    color: "#0c0d10"
                                    border.color: line
                                    implicitHeight: cmd.implicitHeight + 20
                                    Text { id: cmd; anchors.fill: parent; anchors.margins: 10; color: card.step.cmd.indexOf("accord") >= 0 ? accent : fg
                                        font.family: "Menlo"; font.pixelSize: 12; wrapMode: Text.Wrap
                                        text: (card.step.who ? "# next: " + card.step.who + "\n" : "") + card.step.cmd }
                                }
                            }
                        }
                    }
                    Item { height: 10 }
                }
            }
        }
    }
}
