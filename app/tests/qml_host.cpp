// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Runs the app's real view (src/qml/Main.qml) outside Basecamp, against the
// real chain reader, with a stand-in for what Basecamp gives a view: a
// `logos` object whose module("accord") is a backend with the slots of
// src/accord.rep and whose watch() resolves at once. For screenshots and
// layout checks; any QML warning fails the run.
//
//   qml_host <Main.qml> <width> <height> <out.png> --rpc URL --program B58 --multisig ID
//            [--name KEYHEX=label ...] [package.json ...]
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QJSValue>
#include <QJsonDocument>
#include <QJsonObject>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickItemGrabResult>
#include <QQuickWindow>
#include <QTimer>
#include <cstdio>
#include <cstdlib>
#include <functional>

#include "accord_chain.h"

class Backend : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString rpc MEMBER rpc_ CONSTANT)
    Q_PROPERTY(QString program MEMBER program_ CONSTANT)
    Q_PROPERTY(QString appVersion READ appVersion CONSTANT)
    Q_PROPERTY(QVariantMap names MEMBER names_ CONSTANT)
    Q_PROPERTY(QString initialMultisig MEMBER initialMultisig_ CONSTANT)
    Q_PROPERTY(QStringList initialPackages MEMBER initialPackages_ CONSTANT)
public:
    QString rpc_, program_, initialMultisig_;
    QStringList initialPackages_;
    QVariantMap names_;
    accord::Chain chain;
    std::function<void()> onAnswer;
    QString appVersion() const { return QStringLiteral("0.1.0"); }
    void reply(const QString& t, const QJsonObject& o) {
        emit answered(t, QString::fromUtf8(QJsonDocument(o).toJson(QJsonDocument::Compact)));
        if (onAnswer) onAnswer();
    }
    Q_INVOKABLE QString loadMultisig(QString t, QString id) {
        chain.multisig(id, [=, this](const QJsonObject& o) { reply(t, o); });
        return {};
    }
    Q_INVOKABLE QString loadPackage(QString t, QString id, QString path) {
        QFile f(path);
        if (!f.open(QIODevice::ReadOnly)) return "error: cannot read " + path;
        QJsonObject p = accord::readPackage(f.readAll());
        p["file"] = QFileInfo(path).fileName();
        chain.queued(id, QByteArray::fromHex(p["digest"].toString().toLatin1()), [=, this](const QJsonObject& q) mutable {
            p["chain"] = q;
            reply(t, p);
        });
        return {};
    }
    Q_INVOKABLE QString saveSettings(QString, QString) { return {}; }
    // The room needs Basecamp's chat module; the test host has none.
    Q_PROPERTY(QString chatState MEMBER chatState_ CONSTANT)
    QString chatState_ = QStringLiteral("off");
    Q_INVOKABLE QString chatStart() { return QStringLiteral("error: no chat module in the test host"); }
signals:
    void answered(QString token, QString json);
};

class Logos : public QObject {
    Q_OBJECT
public:
    QObject* backend = nullptr;
    Q_INVOKABLE QObject* module(const QString&) { return backend; }
    Q_INVOKABLE bool isViewModuleReady(const QString&) { return true; }
    Q_INVOKABLE void watch(const QJSValue& value, QJSValue ok, QJSValue) { if (ok.isCallable()) ok.call({value}); }
signals:
    void viewModuleReadyChanged(QString name, bool ready);
};

static int qmlWarnings = 0;
static void onMessage(QtMsgType t, const QMessageLogContext&, const QString& m) {
    if (t != QtDebugMsg && t != QtInfoMsg) {
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
    for (int i = 5; i < a.size(); ++i) {
        if (a[i] == "--rpc") be.rpc_ = be.chain.rpc = a[++i];
        else if (a[i] == "--program") { be.program_ = a[++i]; be.chain.program = accord::unbase58(be.program_); }
        else if (a[i] == "--multisig") be.initialMultisig_ = a[++i];
        else if (a[i] == "--name") { auto kv = a[++i].split('='); be.names_[kv[0]] = kv[1]; }
        else be.initialPackages_ << QFileInfo(a[i]).absoluteFilePath();
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
    int expected = 1 + int(be.initialPackages_.size());
    be.onAnswer = [&] {
        if (--expected > 0) return;
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
    QTimer::singleShot(60000, [&] { std::fprintf(stderr, "timed out\n"); std::_Exit(3); });
    return app.exec();
}

#include "qml_host.moc"
