// SPDX-License-Identifier: MIT OR Apache-2.0
#include "accord_chain.h"

#include <QCryptographicHash>
#include <QJsonDocument>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QUrl>
#include <stdexcept>

namespace accord {

static const char* B58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
static const QString NATIVE = QStringLiteral("11111111111111111111111111111111");

QByteArray sha256(const QByteArray& b) { return QCryptographicHash::hash(b, QCryptographicHash::Sha256); }

QString base58(const QByteArray& in) {
    QByteArray b = in;
    int zeros = 0;
    while (zeros < b.size() && b[zeros] == 0) ++zeros;
    QByteArray out;
    QByteArray num = b.mid(zeros);
    while (!num.isEmpty()) {
        QByteArray q;
        int rem = 0;
        for (unsigned char c : num) {
            int acc = rem * 256 + c;
            int d = acc / 58;
            rem = acc % 58;
            if (!q.isEmpty() || d) q.append(char(d));
        }
        out.prepend(B58[rem]);
        num = q;
    }
    return QString(zeros, '1') + QString::fromLatin1(out);
}

QByteArray unbase58(const QString& s) {
    QByteArray out;
    for (QChar ch : s) {
        const char* p = strchr(B58, ch.toLatin1());
        if (!p || !ch.toLatin1()) return {};
        int carry = int(p - B58);
        for (int i = out.size() - 1; i >= 0; --i) {
            carry += 58 * (unsigned char)out[i];
            out[i] = char(carry & 0xff);
            carry >>= 8;
        }
        while (carry) { out.prepend(char(carry & 0xff)); carry >>= 8; }
    }
    int zeros = 0;
    while (zeros < s.size() && s[zeros] == '1') ++zeros;
    return QByteArray(zeros, 0) + out;
}

QByteArray id32(const QString& s) {
    if (s.size() == 64) {
        QByteArray h = QByteArray::fromHex(s.toLatin1());
        if (h.size() == 32) return h;
    }
    QByteArray t = s.toUtf8();
    if (t.size() > 32) return {};
    t.append(QByteArray(32 - t.size(), 0));
    return t;
}

static QByteArray pad32(const char* s) {
    QByteArray b(s);
    b.append(QByteArray(32 - b.size(), 0));
    return b;
}

QByteArray publicPda(const QByteArray& program, const QByteArray& seed) {
    return sha256(pad32("/LEE/v0.2/AccountId/PDA/") + program + seed);
}

QByteArray combineSeeds(const QList<QByteArray>& seeds) {
    if (seeds.size() == 1) return seeds[0];
    QByteArray all;
    for (const auto& s : seeds) all += s;
    return sha256(all);
}

QByteArray configAccount(const QByteArray& p, const QByteArray& id) { return publicPda(p, combineSeeds({id})); }
QByteArray vaultAccount(const QByteArray& p, const QByteArray& id) { return publicPda(p, combineSeeds({id, pad32("vault")})); }
QByteArray queuedAccount(const QByteArray& p, const QByteArray& id, const QByteArray& d) { return publicPda(p, combineSeeds({id, d})); }

static QByteArray le(quint64 v, int n) {
    QByteArray b(n, 0);
    for (int i = 0; i < n; ++i) b[i] = char((v >> (8 * i)) & 0xff);
    return b;
}

QByteArray actionDigest(const QByteArray& config, quint32 epoch, quint64 exp, const QByteArray& action) {
    return sha256(QByteArray("accord_multisig/v1/action") + config + le(epoch, 4) + le(exp, 8) + action);
}

// ------------------------------------------------------------------ borsh

namespace {
struct R {
    const QByteArray& b;
    int i = 0;
    QByteArray take(int n) {
        if (n < 0 || i + n > b.size()) throw std::runtime_error("ran out of bytes");
        QByteArray o = b.mid(i, n);
        i += n;
        return o;
    }
    quint64 u(int n) {
        QByteArray x = take(n);
        quint64 v = 0;
        for (int k = n - 1; k >= 0; --k) v = (v << 8) | (unsigned char)x[k];
        return v;
    }
    QString u128() {
        QByteArray x = take(16);
        // decimal of a little-endian 128-bit value
        QByteArray be;
        for (int k = 15; k >= 0; --k) be.append(x[k]);
        QString out;
        while (true) {
            bool zero = true;
            int rem = 0;
            for (int k = 0; k < 16; ++k) {
                int acc = rem * 256 + (unsigned char)be[k];
                be[k] = char(acc / 10);
                rem = acc % 10;
                if (be[k]) zero = false;
            }
            out.prepend(QChar('0' + rem));
            if (zero) break;
        }
        return out;
    }
    QString hex(int n) { return QString::fromLatin1(take(n).toHex()); }
    QString acc() { return base58(take(32)); }
    quint32 len() { return quint32(u(4)); }
};

QJsonArray roleNames(int r) {
    QJsonArray a;
    if (r & 1) a.append("propose");
    if (r & 2) a.append("approve");
    if (r & 4) a.append("execute");
    return a;
}

QJsonObject member(R& r) {
    QJsonObject m;
    m["key"] = r.hex(32);
    int roles = int(r.u(1));
    m["roles"] = roleNames(roles);
    m["roles_bits"] = roles;
    m["weight"] = double(r.u(4));
    return m;
}

QJsonObject asset(R& r) {
    int t = int(r.u(1));
    if (t == 0) return {{"kind", "native"}};
    if (t == 1) return {{"kind", "token"}, {"definition", r.acc()}};
    throw std::runtime_error("bad asset");
}

QJsonObject limitSpec(R& r) {
    QJsonObject l;
    QJsonArray sp;
    for (quint32 n = r.len(), k = 0; k < n; ++k) sp.append(r.hex(32));
    l["spenders"] = sp;
    l["min_signers"] = double(r.u(1));
    l["asset"] = asset(r);
    l["amount"] = r.u128();
    l["period_ms"] = double(r.u(8));
    return l;
}

QJsonObject config(R& r) {
    QJsonObject c;
    QJsonArray ms;
    for (quint32 n = r.len(), k = 0; k < n; ++k) ms.append(member(r));
    c["members"] = ms;
    c["threshold"] = double(r.u(8));
    c["time_lock_ms"] = double(r.u(8));
    QJsonArray ls;
    for (quint32 n = r.len(), k = 0; k < n; ++k) ls.append(limitSpec(r));
    c["limits"] = ls;
    return c;
}
} // namespace

QJsonObject decodeMultisig(const QByteArray& b) {
    R r{b};
    QJsonObject m;
    m["version"] = double(r.u(1));
    m["initialised"] = r.u(1) != 0;
    m["epoch"] = double(r.u(4));
    QJsonArray ms;
    for (quint32 n = r.len(), k = 0; k < n; ++k) ms.append(member(r));
    m["members"] = ms;
    m["threshold"] = double(r.u(8));
    m["time_lock_ms"] = double(r.u(8));
    QJsonArray ls;
    for (quint32 n = r.len(), k = 0; k < n; ++k) {
        QJsonObject l = limitSpec(r);
        l["period_start"] = double(r.u(8));
        l["spent"] = r.u128();
        l["uses"] = double(r.u(8));
        ls.append(l);
    }
    m["limits"] = ls;
    m["vault"] = r.acc();
    m["creator"] = r.acc();
    m["queued"] = double(r.u(8));
    m["executed"] = double(r.u(8));
    if (r.i != b.size()) throw std::runtime_error("trailing bytes in the configuration");
    return m;
}

QJsonObject decodeQueued(const QByteArray& b) {
    R r{b};
    QJsonObject q;
    q["digest"] = r.hex(32);
    q["kind"] = double(r.u(1));
    q["epoch"] = double(r.u(4));
    q["quorum_at"] = double(r.u(8));
    q["expires_at"] = double(r.u(8));
    q["time_lock_ms"] = double(r.u(8));
    q["proposer"] = r.hex(32);
    QJsonArray ap;
    for (quint32 n = r.len(), k = 0; k < n; ++k) ap.append(r.hex(32));
    q["approvers"] = ap;
    int st = int(r.u(1));
    q["status"] = st == 1 ? "queued" : st == 2 ? "executed" : st == 3 ? "cancelled" : "unknown";
    q["executed_at"] = double(r.u(8));
    if (r.i != b.size()) throw std::runtime_error("trailing bytes in the queued record");
    q["executable_at"] = double(executableAt(q));
    return q;
}

quint64 executableAt(const QJsonObject& q) {
    const quint64 window = 600000;
    quint64 lock = quint64(q["time_lock_ms"].toDouble());
    quint64 at = quint64(q["quorum_at"].toDouble());
    return lock == 0 ? at : at + window + lock;
}

QJsonObject decodeAction(const QByteArray& b, const QByteArray& program) {
    R r{b};
    QJsonObject a;
    int tag = int(r.u(1));
    if (tag == 0) {
        a["kind"] = "calls";
        QJsonArray calls;
        for (quint32 n = r.len(), k = 0; k < n; ++k) {
            QJsonObject c;
            QByteArray prog = r.take(32);
            c["program"] = base58(prog);
            QJsonArray accs;
            for (quint32 m = r.len(), j = 0; j < m; ++j) {
                QString who = r.acc();
                QByteArray sp = r.take(32);
                QString shard = sp == QByteArray(32, 0) ? "native" : sp == program ? "this program" : base58(sp);
                accs.append(QJsonObject{{"account", who}, {"shard", shard}});
            }
            c["accounts"] = accs;
            QByteArray data = r.take(int(r.len()));
            c["data_hex"] = QString::fromLatin1(data.toHex());
            if (prog == QByteArray(32, 0) && data.size() == 17 && data[0] == 0) {
                R d{data};
                d.take(1);
                c["decoded"] = QJsonObject{{"program", "native token"}, {"instruction", "Transfer"}, {"amount", d.u128()},
                                           {"from", accs.size() > 0 ? accs[0].toObject()["account"] : QJsonValue()},
                                           {"to", accs.size() > 1 ? accs[1].toObject()["account"] : QJsonValue()}};
            } else {
                c["warning"] = "This program is not in the registry: the action cannot be decoded.";
            }
            calls.append(c);
        }
        a["calls"] = calls;
    } else if (tag == 1) {
        a["kind"] = "configuration change";
        a["config"] = config(r);
    } else if (tag == 2) {
        a["kind"] = "veto a queued action";
        a["digest"] = r.hex(32);
    } else {
        throw std::runtime_error("unknown action");
    }
    if (r.i != b.size()) throw std::runtime_error("trailing bytes in the action");
    return a;
}

QJsonObject readPackage(const QByteArray& json) {
    QJsonObject p = QJsonDocument::fromJson(json).object();
    if (p.isEmpty()) return {{"ok", false}, {"error", "not an approval package"}};
    QByteArray program = unbase58(p["program"].toString());
    QByteArray id = QByteArray::fromHex(p["multisig_id"].toString().toLatin1());
    QByteArray action = QByteArray::fromHex(p["action"].toString().toLatin1());
    QByteArray cfg = configAccount(program, id);
    QByteArray d = actionDigest(cfg, quint32(p["epoch"].toDouble()), quint64(p["expires_at"].toDouble()), action);
    QJsonObject out = p;
    out["ok"] = true;
    out["digest_matches"] = QString::fromLatin1(d.toHex()) == p["digest"].toString() && base58(cfg) == p["config_account"].toString();
    try {
        out["decoded"] = decodeAction(action, program);
    } catch (const std::exception& e) {
        out["ok"] = false;
        out["error"] = QString::fromUtf8(e.what());
    }
    return out;
}

// ------------------------------------------------------------------ chain

void Chain::call(const QString& method, const QJsonArray& params, std::function<void(const QJsonValue&, const QString&)> done) {
    QNetworkRequest req{QUrl(rpc)};
    req.setHeader(QNetworkRequest::ContentTypeHeader, "application/json");
    req.setTransferTimeout(20000);
    QJsonObject body{{"jsonrpc", "2.0"}, {"id", 1}, {"method", method}, {"params", params}};
    QNetworkReply* reply = nam_.post(req, QJsonDocument(body).toJson(QJsonDocument::Compact));
    connect(reply, &QNetworkReply::finished, this, [reply, done] {
        reply->deleteLater();
        if (reply->error() != QNetworkReply::NoError) return done({}, reply->errorString());
        QJsonObject o = QJsonDocument::fromJson(reply->readAll()).object();
        if (o.contains("error")) return done({}, o["error"].toObject()["message"].toString());
        done(o["result"], {});
    });
}

void Chain::shard(const QByteArray& account, const QString& programB58, std::function<void(QByteArray, QString)> done) {
    call("getAccount", {base58(account)}, [=](const QJsonValue& v, const QString& err) {
        if (!err.isEmpty()) return done({}, err);
        QJsonObject shards = v.toObject()["data"].toObject()["shards"].toObject();
        QByteArray out;
        for (const auto& x : shards[programB58].toArray()) out.append(char(x.toInt()));
        done(out, {});
    });
}

void Chain::now(std::function<void(quint64)> done) {
    call("getAccount", {base58(QByteArray("/LEZ/ClockProgramAccount/0000001"))}, [=](const QJsonValue& v, const QString&) {
        QJsonObject shards = v.toObject()["data"].toObject()["shards"].toObject();
        for (const auto& k : shards.keys()) {
            if (k == NATIVE) continue;
            QByteArray b;
            for (const auto& x : shards[k].toArray()) b.append(char(x.toInt()));
            if (b.size() >= 16) {
                R r{b};
                r.u(8);
                return done(r.u(8));
            }
        }
        done(0);
    });
}

void Chain::multisig(const QString& idText, Done done) {
    QByteArray id = id32(idText);
    if (id.size() != 32) return done({{"ok", false}, {"error", "an id is 64 hex digits or at most 32 bytes of text"}});
    QByteArray cfg = configAccount(program, id);
    shard(cfg, base58(program), [=, this](QByteArray b, QString err) {
        if (!err.isEmpty()) return done({{"ok", false}, {"error", err}});
        if (b.isEmpty()) return done({{"ok", false}, {"error", "No multisig at this id (E7100)."}});
        QJsonObject m;
        try {
            m = decodeMultisig(b);
        } catch (const std::exception& e) {
            return done({{"ok", false}, {"error", QString::fromUtf8(e.what())}});
        }
        m["ok"] = true;
        m["multisig_id"] = QString::fromLatin1(id.toHex());
        m["id_text"] = idText;
        m["configuration"] = base58(cfg);
        call("getAccount", {m["vault"].toString()}, [=, this](const QJsonValue& v, const QString&) mutable {
            QByteArray nb;
            for (const auto& x : v.toObject()["data"].toObject()["shards"].toObject()[NATIVE].toArray()) nb.append(char(x.toInt()));
            R r{nb};
            m["vault_native"] = nb.size() >= 16 ? r.u128() : QStringLiteral("0");
            now([=](quint64 t) mutable {
                m["clock"] = double(t);
                done(m);
            });
        });
    });
}

void Chain::queued(const QString& idText, const QByteArray& digest, Done done) {
    QByteArray id = id32(idText);
    QByteArray q = queuedAccount(program, id, digest);
    shard(q, base58(program), [=, this](QByteArray b, QString err) {
        if (!err.isEmpty()) return done({{"ok", false}, {"error", err}});
        if (b.isEmpty()) return done({{"ok", true}, {"status", "not queued"}, {"account", base58(q)}});
        QJsonObject o;
        try {
            o = decodeQueued(b);
        } catch (const std::exception& e) {
            return done({{"ok", false}, {"error", QString::fromUtf8(e.what())}});
        }
        o["ok"] = true;
        o["account"] = base58(q);
        now([=](quint64 t) mutable {
            o["clock"] = double(t);
            done(o);
        });
    });
}

} // namespace accord
