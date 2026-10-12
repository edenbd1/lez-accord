// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Runs the app's real view (src/qml/Main.qml) outside Basecamp, against the
// real chain reader, with a stand-in for what Basecamp gives a view: a
// `logos` object whose module("accord") is the backend. For screenshots and
// layout checks; any QML warning fails the run.
//
//   qml_host <Main.qml> <width> <height> <out.png> --rpc URL --program B58 --multisig ID
//            [--name KEYHEX=label ...] [package.json ...]
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJsonDocument>
#include <QJsonObject>
#include <QQmlApplicationEngine>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QTimer>
#include <cstdio>
#include <cstdlib>
#include <memory>

#include "accord_chain.h"

class Backend : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString rpc MEMBER rpc_ CONSTANT)
    Q_PROPERTY(QVariantMap names MEMBER names_ CONSTANT)
public:
    QString rpc_;
    QVariantMap names_;
    accord::Chain chain;
    int pending = 0;
signals:
    void multisigLoaded(QString json);
    void packageLoaded(QString json);
};

class Logos : public QObject {
    Q_OBJECT
public:
    QObject* backend = nullptr;
    Q_INVOKABLE QObject* module(const QString&) { return backend; }
};

static int qmlWarnings = 0;
static void onMessage(QtMsgType t, const QMessageLogContext&, const QString& m) {
    if (t != QtDebugMsg) {
        std::fprintf(stderr, "qml: %s\n", qPrintable(m));
        ++qmlWarnings;
    }
}

int main(int argc, char** argv) {
    qInstallMessageHandler(onMessage);
    QGuiApplication app(argc, argv);
    QStringList a = app.arguments();
    if (a.size() < 5) {
        std::fprintf(stderr, "usage: qml_host <Main.qml> <w> <h> <out.png> --rpc URL --program B58 --multisig ID [--name K=label] [packages]\n");
        return 2;
    }
    const QString qml = a[1], out = a[4];
    const int w = a[2].toInt(), h = a[3].toInt();
    Backend be;
    QString multisig;
    QStringList packages;
    for (int i = 5; i < a.size(); ++i) {
        if (a[i] == "--rpc") be.rpc_ = be.chain.rpc = a[++i];
        else if (a[i] == "--program") be.chain.program = accord::unbase58(a[++i]);
        else if (a[i] == "--multisig") multisig = a[++i];
        else if (a[i] == "--name") { auto kv = a[++i].split('='); be.names_[kv[0]] = kv[1]; }
        else packages << a[i];
    }
    Logos logos;
    logos.backend = &be;

    QQuickWindow window;
    window.resize(w, h);
    QQmlEngine engine;
    engine.rootContext()->setContextProperty("logos", &logos);
    QQmlComponent comp(&engine, QUrl::fromLocalFile(QFileInfo(qml).absoluteFilePath()));
    QObject* obj = comp.create(engine.rootContext());
    if (!obj) {
        for (const auto& e : comp.errors()) std::fprintf(stderr, "qml: %s\n", qPrintable(e.toString()));
        return 1;
    }
    auto* item = qobject_cast<QQuickItem*>(obj);
    item->setParentItem(window.contentItem());
    item->setSize(QSizeF(w, h));
    window.show();

    be.pending = 1 + int(packages.size());
    auto finish = [&] {
        if (--be.pending > 0) return;
        QTimer::singleShot(1500, [&] {
            auto grab = item->grabToImage();
            QObject::connect(grab.data(), &QQuickItemGrabResult::ready, [&, grab] {
                grab->image().save(out);
                std::printf("%s: %dx%d, %d QML warning(s)\n", qPrintable(out), w, h, qmlWarnings);
                std::fflush(stdout);
                std::_Exit(qmlWarnings ? 1 : 0);
            });
        });
    };
    be.chain.multisig(multisig, [&](const QJsonObject& o) {
        emit be.multisigLoaded(QString::fromUtf8(QJsonDocument(o).toJson(QJsonDocument::Compact)));
        finish();
    });
    int index = 0;
    for (const auto& path : packages) {
        QFile f(path);
        if (!f.open(QIODevice::ReadOnly)) { std::fprintf(stderr, "cannot read %s\n", qPrintable(path)); return 2; }
        QJsonObject p = accord::readPackage(f.readAll());
        p["file"] = QFileInfo(path).fileName();
        p["index"] = index++;
        be.chain.queued(multisig, QByteArray::fromHex(p["digest"].toString().toLatin1()), [&, p](const QJsonObject& q) mutable {
            p["chain"] = q;
            emit be.packageLoaded(QString::fromUtf8(QJsonDocument(p).toJson(QJsonDocument::Compact)));
            finish();
        });
    }
    QTimer::singleShot(60000, [&] { std::fprintf(stderr, "timed out\n"); app.exit(3); });
    return app.exec();
}

#include "qml_host.moc"
