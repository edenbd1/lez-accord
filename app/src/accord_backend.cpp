// SPDX-License-Identifier: MIT OR Apache-2.0
#include "accord_backend.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QStandardPaths>

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
