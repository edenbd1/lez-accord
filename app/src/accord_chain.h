// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Reads Accord multisigs from a LEZ v0.3 sequencer and decodes approval
// packages, with plain Qt (Core, Network). Holds no keys and signs nothing.
//
// Everything decoded here is decoded from bytes: the configuration and
// queued-record shards from the chain (borsh, the layout of
// programs/accord/core/src/lib.rs and idl/accord_multisig.idl.json), and a
// package's action from the exact bytes its approvals sign, whose digest is
// recomputed and compared before anything is shown.
#pragma once

#include <QByteArray>
#include <QJsonArray>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QObject>
#include <QString>
#include <functional>

namespace accord {

QByteArray sha256(const QByteArray& b);
QString base58(const QByteArray& b);
QByteArray unbase58(const QString& s);
/// A 32-byte id: 64 hex digits, or text of at most 32 bytes, zero-padded.
QByteArray id32(const QString& s);
QByteArray publicPda(const QByteArray& program, const QByteArray& seed);
QByteArray combineSeeds(const QList<QByteArray>& seeds);
QByteArray configAccount(const QByteArray& program, const QByteArray& multisigId);
QByteArray vaultAccount(const QByteArray& program, const QByteArray& multisigId);
QByteArray queuedAccount(const QByteArray& program, const QByteArray& multisigId, const QByteArray& digest);
QByteArray actionDigest(const QByteArray& config, quint32 epoch, quint64 expiresAt, const QByteArray& actionBytes);

/// Borsh decoders; each throws std::runtime_error on short or malformed bytes.
QJsonObject decodeMultisig(const QByteArray& b);
QJsonObject decodeQueued(const QByteArray& b);
QJsonObject decodeAction(const QByteArray& b, const QByteArray& program);
quint64 executableAt(const QJsonObject& queued);

/// Package file → {ok, error | digest, digest_matches, action, approvals, ...}.
QJsonObject readPackage(const QByteArray& json);

class Chain : public QObject {
    Q_OBJECT
public:
    QString rpc;
    QByteArray program;
    using Done = std::function<void(const QJsonObject&)>;

    /// The configuration, the vault's native balance and the chain's clock.
    void multisig(const QString& id, Done done);
    /// A queued record, by multisig id and digest, with what the clock allows.
    void queued(const QString& id, const QByteArray& digest, Done done);
    void now(std::function<void(quint64)> done);

private:
    void call(const QString& method, const QJsonArray& params, std::function<void(const QJsonValue&, const QString&)> done);
    void shard(const QByteArray& account, const QString& programB58, std::function<void(QByteArray, QString)> done);
    QNetworkAccessManager nam_;
};

} // namespace accord
