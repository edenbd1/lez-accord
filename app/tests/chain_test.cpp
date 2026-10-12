// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The app's reader checked against the Rust program, offline:
//   - every stored-account vector the core crate emits (idl_vectors.json) is
//     decoded by the C++ decoder and compared field by field;
//   - PDAs (configuration, vault, queued record) match what the CLI derives;
//   - a real approval package from the live room run decodes, and its
//     recomputed digest matches; a package with one flipped byte does not.
//
//   chain_test <fixtures dir>
#include <QCoreApplication>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <cstdio>

#include "accord_chain.h"

static int fails = 0;
static void check(bool ok, const QString& what) {
    std::printf("  %s %s\n", ok ? "ok  " : "FAIL", qPrintable(what));
    if (!ok) ++fails;
}

static QByteArray read(const QString& p) {
    QFile f(p);
    if (!f.open(QIODevice::ReadOnly)) return {};
    return f.readAll();
}

int main(int argc, char** argv) {
    QCoreApplication app(argc, argv);
    const QString dir = argc > 1 ? argv[1] : "app/tests/fixtures";
    QJsonObject vec = QJsonDocument::fromJson(read(dir + "/idl_vectors.json")).object();
    for (const auto& v : vec["accounts"].toArray()) {
        QJsonObject s = v.toObject();
        QByteArray b = QByteArray::fromHex(s["hex"].toString().toLatin1());
        QJsonObject want = s["values"].toObject();
        if (s["name"] == "Multisig") {
            QJsonObject m = accord::decodeMultisig(b);
            check(m["epoch"].toInt() == want["epoch"].toInt() && m["threshold"].toInt() == want["threshold"].toInt() &&
                      m["time_lock_ms"].toInt() == want["time_lock_ms"].toInt() && m["queued"].toInt() == want["queued"].toInt() &&
                      m["executed"].toInt() == want["executed"].toInt(),
                  "Multisig scalars decode as the program wrote them");
            QJsonObject m0 = m["members"].toArray()[0].toObject(), w0 = want["members"].toArray()[0].toObject();
            check(m0["key"] == w0["key"] && m0["roles_bits"].toInt() == w0["roles"].toInt() && m0["weight"].toInt() == w0["weight"].toInt(),
                  "Multisig members decode");
            QJsonObject l = m["limits"].toArray()[0].toObject(), wl = want["limits"].toArray()[0].toObject();
            check(l["spent"].toString() == wl["spent"].toString() && l["uses"].toInt() == wl["uses"].toInt() &&
                      l["amount"].toString() == wl["spec"].toObject()["amount"].toString(),
                  "Multisig limits decode, u128 included");
            check(accord::unbase58(m["vault"].toString()).toHex() == want["vault"].toString().toLatin1(), "Multisig vault decodes");
        } else if (s["name"] == "Queued") {
            QJsonObject q = accord::decodeQueued(b);
            check(q["digest"] == want["digest"] && q["quorum_at"].toInt() == want["quorum_at"].toInt() &&
                      q["expires_at"].toInt() == want["expires_at"].toInt() && q["time_lock_ms"].toInt() == want["time_lock_ms"].toInt() &&
                      q["status"] == "queued" && q["approvers"].toArray()[0] == want["approvers"].toArray()[0],
                  "Queued decodes as the program wrote it");
            check(qint64(q["executable_at"].toDouble()) == 107 + 600000 + 113, "executable_at counts the lock from the window's end");
        }
    }
    bool threw = false;
    try {
        accord::decodeMultisig(QByteArray(10, 1));
    } catch (...) {
        threw = true;
    }
    check(threw, "a short configuration is refused, not misread");

    QJsonObject ids = QJsonDocument::fromJson(read(dir + "/ids.json")).object();
    QByteArray prog = accord::unbase58("HiuknECjmrx1ToZuGGsCVfs99N21VjYnrb8ZqBXphorg");
    QByteArray id = accord::id32("treasury");
    QByteArray dg = QByteArray::fromHex("284d0b3fd4609ab20c8736f53fad1f3a5fe76803a2c8a2f90a0cef66ad12b9c3");
    check(accord::base58(accord::configAccount(prog, id)) == ids["configuration"].toString(), "configuration PDA matches the CLI");
    check(accord::base58(accord::vaultAccount(prog, id)) == ids["vault"].toString(), "vault PDA matches the CLI");
    check(accord::base58(accord::queuedAccount(prog, id, dg)) == ids["queued"].toString(), "queued PDA matches the CLI");

    QByteArray pkgBytes = read(dir + "/package.json");
    QJsonObject pkg = accord::readPackage(pkgBytes);
    check(pkg["ok"].toBool() && pkg["digest_matches"].toBool(), "a package from the live room run: digest recomputed from its bytes");
    QJsonObject call = pkg["decoded"].toObject()["calls"].toArray()[0].toObject();
    check(call["decoded"].toObject()["amount"].toString() == "70", "its action decodes: a native transfer of 70");
    QJsonObject tampered = QJsonDocument::fromJson(pkgBytes).object();
    QByteArray act = QByteArray::fromHex(tampered["action"].toString().toLatin1());
    act[act.size() - 1] = char(act[act.size() - 1] ^ 1);
    tampered["action"] = QString::fromLatin1(act.toHex());
    QJsonObject bad = accord::readPackage(QJsonDocument(tampered).toJson());
    check(!bad["digest_matches"].toBool(), "a package with one byte of its action changed fails the digest check");

    std::printf("%s: %d failure(s)\n", fails ? "FAIL" : "ok", fails);
    return fails ? 1 : 0;
}
