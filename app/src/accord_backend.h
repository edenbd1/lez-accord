// SPDX-License-Identifier: MIT OR Apache-2.0
//
// The Basecamp side of Accord. It reads the chain and decodes approval
// packages through accord::Chain (accord_chain.h, tested on its own) and hands
// the view JSON. It holds no keys and signs nothing.
#pragma once

#include <QJsonObject>
#include <QString>

#include "accord_chain.h"
#include "logos_ui_plugin_context.h"
#include "rep_accord_source.h"

class AccordBackend : public AccordSimpleSource, public LogosUiPluginContext {
public:
    AccordBackend();
    QString loadMultisig(QString token, QString multisigId) override;
    QString loadPackage(QString token, QString multisigId, QString pathOrJson) override;
    QString saveSettings(QString rpc, QString program) override;
    QString chatStart() override;
    QString roomCreate(QString token, QString name) override;
    QString roomAdd(QString convoId, QString address) override;
    QString roomPost(QString convoId, QString textOrPackage) override;
    QString roomList(QString token) override;

protected:
    void onContextReady() override;

private:
    bool chatUp_ = false;
    QString dataDir() const;
    void apply(const QString& rpc, const QString& program);
    void reply(const QString& token, const QJsonObject& json);
    accord::Chain chain_;
};
