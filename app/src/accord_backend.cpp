// SPDX-License-Identifier: MIT OR Apache-2.0
#include "accord_backend.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonArray>
#include <QTimer>
#include <QStandardPaths>

// Generated umbrella: LogosModules (behind modules()) from metadata.json's dependencies.
#include "logos_sdk.h"

namespace {
const char* kDefaultRpc = "https://testnet.lez.logos.co";
QString envOr(const char* name, const QString& fallback) {
    const QString v = qEnvironmentVariable(name).trimmed();
    return v.isEmpty() ? fallback : v;
}
}  // namespace

AccordBackend::AccordBackend() {
    setAppVersion(QStringLiteral(ACCORD_VERSION));
    QJsonObject saved;
    QFile f(dataDir() + QStringLiteral("/settings.json"));
    if (f.open(QIODevice::ReadOnly)) saved = QJsonDocument::fromJson(f.readAll()).object();
    apply(envOr("ACCORD_RPC", saved["rpc"].toString(kDefaultRpc)), envOr("ACCORD_PROGRAM", saved["program"].toString()));
}

QString AccordBackend::dataDir() const {
    const QString userDir = qEnvironmentVariable("LOGOS_USER_DIR");
    const QString dir = userDir.isEmpty()
        ? QStandardPaths::writableLocation(QStandardPaths::AppDataLocation) + QStringLiteral("/accord")
        : userDir + QStringLiteral("/module_data/accord");
    QDir().mkpath(dir);
    return dir;
}

void AccordBackend::apply(const QString& rpc, const QString& program) {
    chain_.rpc = rpc;
    chain_.program = accord::unbase58(program);
    setRpc(rpc);
    setProgram(program);
}

void AccordBackend::reply(const QString& token, const QJsonObject& json) {
    emit answered(token, QString::fromUtf8(QJsonDocument(json).toJson(QJsonDocument::Compact)));
}

QString AccordBackend::loadMultisig(QString token, QString multisigId) {
    if (chain_.program.size() != 32) return QStringLiteral("error: set the program's header account in Settings");
    chain_.multisig(multisigId.trimmed(), [this, token](const QJsonObject& o) { reply(token, o); });
    return {};
}

QString AccordBackend::loadPackage(QString token, QString multisigId, QString pathOrJson) {
    QByteArray bytes = pathOrJson.trimmed().toUtf8();
    if (!bytes.startsWith('{')) {
        QFile f(QString::fromUtf8(bytes));
        if (!f.open(QIODevice::ReadOnly)) return QStringLiteral("error: cannot read ") + pathOrJson;
        bytes = f.readAll();
    }
    QJsonObject p = accord::readPackage(bytes);
    if (!p["ok"].toBool()) return QStringLiteral("error: ") + p["error"].toString();
    p["file"] = pathOrJson.trimmed().startsWith('{') ? QStringLiteral("package.json") : QFileInfo(pathOrJson.trimmed()).fileName();
    chain_.queued(multisigId.trimmed(), QByteArray::fromHex(p["digest"].toString().toLatin1()), [this, token, p](const QJsonObject& q) mutable {
        p["chain"] = q;
        reply(token, p);
    });
    return {};
}

QString AccordBackend::saveSettings(QString rpc, QString program) {
    rpc = rpc.trimmed().isEmpty() ? QString::fromLatin1(kDefaultRpc) : rpc.trimmed();
    if (!program.trimmed().isEmpty() && accord::unbase58(program.trimmed()).size() != 32) return QStringLiteral("error: not a base58 account id");
    QFile f(dataDir() + QStringLiteral("/settings.json"));
    if (!f.open(QIODevice::WriteOnly)) return QStringLiteral("error: cannot save settings");
    f.write(QJsonDocument(QJsonObject{{"rpc", rpc}, {"program", program.trimmed()}}).toJson());
    apply(rpc, program.trimmed());
    return {};
}

// ── The room ───────────────────────────────────────────────────────────────
//
// A Logos Chat group conversation (MLS, end-to-end encrypted, over Logos
// Delivery) per multisig per configuration epoch. Messages are text: a line of
// deliberation, or an approval package's JSON, which the view decodes from its
// bytes exactly as it decodes a package opened from a file. The chat module is
// a transport here: the program verifies every approval again.

void AccordBackend::onContextReady() {}

// The chat address, read outside the event callback that announced the node,
// and retried while the module has not settled it yet.
void AccordBackend::refreshAddress(int tries) {
    const QString a = modules().chat_module.get_address();
    if (a.isEmpty()) {
        if (tries > 0) QTimer::singleShot(2000, this, [this, tries] { refreshAddress(tries - 1); });
        return;
    }
    setChatAddress(a);
    // Also on disk, so a member can hand it to the others from a script.
    QFile f(dataDir() + QStringLiteral("/chat_address"));
    if (f.open(QIODevice::WriteOnly)) f.write(a.toUtf8());
}

QString AccordBackend::chatStart() {
    if (chatUp_) return {};
    auto& chat = modules().chat_module;
    chat.on(QStringLiteral("message_received"), [this](const QVariantList& a) {
        emit roomMessage(a.value(0).toString(), a.value(3).toString(), a.value(1).toString(), false);
    });
    chat.on(QStringLiteral("message_sent"), [this](const QVariantList& a) {
        emit roomMessage(a.value(0).toString(), chatAddress(), a.value(1).toString(), true);
    });
    chat.on(QStringLiteral("delivery_state_changed"), [this](const QVariantList& a) {
        setChatState(a.value(0).toString());
        if (a.value(0).toString() == QLatin1String("online")) QTimer::singleShot(0, this, [this] { refreshAddress(5); });
    });
    ChatModule::ChatConfig config;
    config.delivery_preset = envOr("ACCORD_CHAT_PRESET", QStringLiteral("logos.test"));
    config.log_level = QStringLiteral("warn");
    const LogosResult res = chat.init(config);
    if (!res.success) return QStringLiteral("error: the chat module did not start: ") + res.getError<QString>();
    chatUp_ = true;
    setChatState(QStringLiteral("initialising"));
    QTimer::singleShot(1000, this, [this] { refreshAddress(30); });
    return {};
}

QString AccordBackend::roomCreate(QString token, QString name) {
    if (!chatUp_) return QStringLiteral("error: start the room first");
    const LogosResult res = modules().chat_module.create_group_conversation(name, QStringLiteral("Accord coordination room"));
    if (!res.success) return QStringLiteral("error: ") + res.getError<QString>();
    reply(token, QJsonObject{{"ok", true}, {"convo_id", res.getValue<QString>()}});
    return {};
}

QString AccordBackend::roomAdd(QString convoId, QString address) {
    if (!chatUp_) return QStringLiteral("error: start the room first");
    const LogosResult res = modules().chat_module.add_group_member(convoId, address.trimmed());
    return res.success ? QString() : QStringLiteral("error: ") + res.getError<QString>();
}

QString AccordBackend::roomPost(QString convoId, QString textOrPackage) {
    if (!chatUp_) return QStringLiteral("error: start the room first");
    QString content = textOrPackage.trimmed();
    if (!content.startsWith('{') && QFileInfo::exists(content)) {
        QFile f(content);
        if (!f.open(QIODevice::ReadOnly)) return QStringLiteral("error: cannot read ") + content;
        const QJsonObject p = accord::readPackage(f.readAll());
        if (!p["ok"].toBool() || !p["digest_matches"].toBool()) return QStringLiteral("error: not a valid approval package");
        f.seek(0);
        content = QString::fromUtf8(QJsonDocument::fromJson(f.readAll()).toJson(QJsonDocument::Compact));
    }
    const LogosResult res = modules().chat_module.send_message(convoId, content);
    return res.success ? QString() : QStringLiteral("error: ") + res.getError<QString>();
}

QString AccordBackend::roomList(QString token) {
    if (!chatUp_) return QStringLiteral("error: start the room first");
    QJsonArray rooms;
    for (const ChatModule::Conversation& c : modules().chat_module.list_conversations()) {
        if (c.kind != QLatin1String("group") || c.history_only) continue;
        QJsonArray members;
        for (const ChatModule::GroupMember& m : modules().chat_module.list_group_members(c.convo_id))
            members.append(QJsonObject{{"address", m.address}, {"pending", m.pending}});
        rooms.append(QJsonObject{{"convo_id", c.convo_id}, {"name", c.name.value_or(QString())}, {"members", members}});
    }
    reply(token, QJsonObject{{"ok", true}, {"rooms", rooms}});
    return {};
}
