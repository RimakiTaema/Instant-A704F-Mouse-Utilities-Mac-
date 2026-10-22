#include "mainwindow.h"

#include <QCheckBox>
#include <QCloseEvent>
#include <QComboBox>
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileDialog>
#include <QFileInfo>
#include <QFont>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QProcess>
#include <QPushButton>
#include <QSaveFile>
#include <QSettings>
#include <QSlider>
#include <QSpinBox>
#include <QStandardPaths>
#include <QTabWidget>
#include <QTableWidget>
#include <QTableWidgetItem>
#include <QVBoxLayout>

#include <algorithm>
#include <stdexcept>
#if defined(Q_OS_UNIX)
#include <csignal>
#include <sys/types.h>
#include <unistd.h>
#endif

namespace {

const std::array<int, 7> buttonCodes{1, 2, 3, 5, 4, 8, 6};
const std::array<const char *, 7> buttonNames{
    "Left button", "Right button", "Middle button", "Side forward", "Side backward", "Wheel forward", "Wheel backward"};

void writeJsonAtomically(const QString &path, const QByteArray &data)
{
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) {
        throw std::runtime_error(file.errorString().toStdString());
    }
    if (file.write(data) != data.size() || !file.commit()) {
        throw std::runtime_error(file.errorString().toStdString());
    }
}

}

MainWindow::MainWindow(QWidget *parent)
    : QMainWindow(parent)
{
    dataDirectory_ = QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation);
    QDir().mkpath(dataDirectory_);
    buildUi();

    QSettings preferences;
    const QString savedCli = preferences.value("cliPath").toString();
    if (!savedCli.isEmpty()) {
        cliPathEdit_->setText(savedCli);
    }
    if (QFile::exists(settingsPath())) {
        loadSettingsFile(settingsPath());
    }

    connect(&process_, &QProcess::readyReadStandardOutput, this, [this] {
        appendOutput(process_.readAllStandardOutput());
    });
    connect(&process_, &QProcess::readyReadStandardError, this, [this] {
        appendOutput(process_.readAllStandardError());
    });
    connect(&process_, &QProcess::finished, this, &MainWindow::onProcessFinished);
    connect(&process_, &QProcess::errorOccurred, this, &MainWindow::onProcessError);
    connect(&process_, &QProcess::started, this, [this] {
        if (task_ == Task::RunMacro) {
            stopMacroButton_->setEnabled(true);
            statusLabel_->setText("Macro listener active");
        }
    });

    inspectDevice();
}

void MainWindow::buildUi()
{
    setWindowTitle("A704F Configurator");
    resize(880, 760);

    auto *central = new QWidget(this);
    auto *layout = new QVBoxLayout(central);
    auto *header = new QHBoxLayout;
    auto *title = new QLabel("Instant A704F", central);
    QFont titleFont = title->font();
    titleFont.setPointSize(titleFont.pointSize() + 4);
    titleFont.setBold(true);
    title->setFont(titleFont);
    header->addWidget(title);
    header->addStretch();
    statusLabel_ = new QLabel("Starting", central);
    header->addWidget(statusLabel_);
    layout->addLayout(header);

    auto *cliRow = new QHBoxLayout;
    cliRow->addWidget(new QLabel("Rust CLI", central));
    cliPathEdit_ = new QLineEdit(central);
    cliPathEdit_->setText(QString::fromLocal8Bit(qgetenv("A704F_CLI_PATH")));
#ifdef A704F_CLI_DEFAULT_PATH
    if (cliPathEdit_->text().isEmpty()) {
        cliPathEdit_->setText(QStringLiteral(A704F_CLI_DEFAULT_PATH));
    }
#endif
    if (cliPathEdit_->text().isEmpty()) {
        cliPathEdit_->setText(QStandardPaths::findExecutable("a704f-macos"));
    }
    cliRow->addWidget(cliPathEdit_, 1);
    auto *browseCli = new QPushButton("Browse...", central);
    cliRow->addWidget(browseCli);
    layout->addLayout(cliRow);
    connect(browseCli, &QPushButton::clicked, this, [this] {
        const QString path = QFileDialog::getOpenFileName(this, "Select a704f-macos CLI");
        if (!path.isEmpty()) {
            cliPathEdit_->setText(path);
            QSettings().setValue("cliPath", path);
        }
    });
    connect(cliPathEdit_, &QLineEdit::editingFinished, this, [this] {
        QSettings().setValue("cliPath", cliPathEdit_->text().trimmed());
    });

    auto *tabs = new QTabWidget(central);
    buildDevicePage(tabs);
    buildSettingsPage(tabs);
    buildMacroPage(tabs);
    layout->addWidget(tabs, 1);

    auto *logGroup = new QGroupBox("Activity", central);
    auto *logLayout = new QVBoxLayout(logGroup);
    logOutput_ = new QPlainTextEdit(logGroup);
    logOutput_->setReadOnly(true);
    logOutput_->setMaximumBlockCount(800);
    logOutput_->setMaximumHeight(130);
    logLayout->addWidget(logOutput_);
    layout->addWidget(logGroup);

    setCentralWidget(central);
}

void MainWindow::buildDevicePage(QTabWidget *tabs)
{
    auto *page = new QWidget(tabs);
    auto *layout = new QVBoxLayout(page);
    auto *heading = new QLabel("Connected device", page);
    QFont headingFont = heading->font();
    headingFont.setBold(true);
    heading->setFont(headingFont);
    layout->addWidget(heading);

    deviceLabel_ = new QLabel("Not inspected", page);
    deviceLabel_->setWordWrap(true);
    layout->addWidget(deviceLabel_);
    auto *inspect = new QPushButton("Refresh device inspection", page);
    layout->addWidget(inspect, 0, Qt::AlignLeft);
    deviceOutput_ = new QPlainTextEdit(page);
    deviceOutput_->setReadOnly(true);
    layout->addWidget(deviceOutput_, 1);
    layout->addStretch();
    tabs->addTab(page, "Device");
    connect(inspect, &QPushButton::clicked, this, &MainWindow::inspectDevice);
}

void MainWindow::buildSettingsPage(QTabWidget *tabs)
{
    auto *page = new QWidget(tabs);
    auto *layout = new QVBoxLayout(page);
    auto *form = new QFormLayout;

    rgbMode_ = new QComboBox(page);
    const std::array<std::pair<const char *, int>, 8> rgbModes{{
        {"Slow (6 s)", 0x10}, {"Slower (5 s)", 0x11}, {"Slightly slower (4 s)", 0x12},
        {"Fast (3 s)", 0x13}, {"Faster (2 s)", 0x14}, {"Very fast (1 s)", 0x15},
        {"Static", 0x16}, {"Off", 0x17}}};
    for (const auto &[name, value] : rgbModes) {
        rgbMode_->addItem(QString::fromLatin1(name), value);
    }
    form->addRow("RGB mode", rgbMode_);

    scrollMode_ = new QComboBox(page);
    scrollMode_->addItem("Normal scrolling", 0);
    scrollMode_->addItem("Control volume", 1);
    scrollMode_->addItem("Documented debug action", 2);
    form->addRow("Scroll mode", scrollMode_);

    scrollDebug_ = new QComboBox(page);
    const std::array<std::pair<const char *, int>, 15> debugActions{{
        {"Normal scrolling", 0x01}, {"Reverse scrolling", 0x02}, {"Volume", 0x03},
        {"Reverse volume", 0x04}, {"Cycle DPI profile", 0x06}, {"Show desktop", 0x07},
        {"Toggle RGB", 0x08}, {"Flash RGB", 0x09}, {"Reset RGB (purple)", 0x0b},
        {"Reset RGB (blue)", 0x0c}, {"Spam z key", 0x0d}, {"Flash red", 0x10},
        {"Briefly turn RGB off", 0x11}, {"Flash blue", 0x12}, {"Flash magenta", 0x13}}};
    for (const auto &[name, value] : debugActions) {
        scrollDebug_->addItem(QString::fromLatin1(name), value);
    }
    form->addRow("Debug action", scrollDebug_);

    fireRate_ = new QSlider(Qt::Horizontal, page);
    fireRate_->setRange(0, 255);
    fireRate_->setTracking(false);
    fireRate_->setValue(4);
    fireRateLabel_ = new QLabel(page);
    auto *fireRateRow = new QWidget(page);
    auto *fireRateLayout = new QHBoxLayout(fireRateRow);
    fireRateLayout->setContentsMargins(0, 0, 0, 0);
    fireRateLayout->addWidget(fireRate_, 1);
    fireRateLayout->addWidget(fireRateLabel_);
    form->addRow("Fire-rate value", fireRateRow);

    activeDpi_ = new QComboBox(page);
    for (int index = 0; index < 4; ++index) {
        activeDpi_->addItem(QString("Profile %1").arg(index + 1), index);
    }
    form->addRow("Active DPI profile", activeDpi_);
    layout->addLayout(form);

    auto *dpiGroup = new QGroupBox("DPI profiles", page);
    auto *dpiLayout = new QGridLayout(dpiGroup);
    dpiLayout->addWidget(new QLabel("Profile", dpiGroup), 0, 0);
    dpiLayout->addWidget(new QLabel("Value (200 DPI steps)", dpiGroup), 0, 1);
    dpiLayout->addWidget(new QLabel("Enabled", dpiGroup), 0, 2);
    for (int index = 0; index < 4; ++index) {
        dpiLayout->addWidget(new QLabel(QString::number(index + 1), dpiGroup), index + 1, 0);
        dpiValues_[index] = new QSpinBox(dpiGroup);
        dpiValues_[index]->setRange(1, 15);
        dpiValues_[index]->setSuffix(" x200 DPI");
        const std::array<int, 4> defaultDpiValues{3, 4, 6, 12};
        dpiValues_[index]->setValue(defaultDpiValues[index]);
        dpiLayout->addWidget(dpiValues_[index], index + 1, 1);
        dpiEnabled_[index] = new QCheckBox(dpiGroup);
        dpiEnabled_[index]->setChecked(true);
        dpiLayout->addWidget(dpiEnabled_[index], index + 1, 2, Qt::AlignLeft);
    }
    layout->addWidget(dpiGroup);

    auto *bindingsGroup = new QGroupBox("Button bindings", page);
    auto *bindingsLayout = new QGridLayout(bindingsGroup);
    bindingsLayout->addWidget(new QLabel("Physical button", bindingsGroup), 0, 0);
    bindingsLayout->addWidget(new QLabel("Action", bindingsGroup), 0, 1);
    const std::array<std::pair<const char *, int>, 12> actions{{
        {"Key input", 0x00}, {"Left click", 0x01}, {"Middle click", 0x02},
        {"Right click", 0x03}, {"Back", 0x04}, {"Forward", 0x05},
        {"DPI cycle", 0x06}, {"Show desktop", 0x07}, {"Double click", 0x08},
        {"Fire", 0x09}, {"DPI+", 0x0a}, {"DPI-", 0x0b}}};
    for (int index = 0; index < 7; ++index) {
        bindingsLayout->addWidget(new QLabel(QString::fromLatin1(buttonNames[index]), bindingsGroup), index + 1, 0);
        bindings_[index] = new QComboBox(bindingsGroup);
        for (const auto &[name, value] : actions) {
            bindings_[index]->addItem(QString::fromLatin1(name), value);
        }
        const std::array<int, 7> defaultActions{1, 2, 3, 4, 5, 11, 10};
        bindings_[index]->setCurrentIndex(bindings_[index]->findData(defaultActions[index]));
        bindingsLayout->addWidget(bindings_[index], index + 1, 1);
    }
    layout->addWidget(bindingsGroup);

    auto *buttons = new QHBoxLayout;
    auto *importButton = new QPushButton("Import JSON...", page);
    auto *exportButton = new QPushButton("Export JSON...", page);
    auto *plan = new QPushButton("Preview reports", page);
    auto *apply = new QPushButton("Apply to mouse...", page);
    buttons->addWidget(importButton);
    buttons->addWidget(exportButton);
    buttons->addStretch();
    buttons->addWidget(plan);
    buttons->addWidget(apply);
    layout->addLayout(buttons);
    layout->addStretch();
    tabs->addTab(page, "Settings");

    connect(scrollMode_, qOverload<int>(&QComboBox::currentIndexChanged), this, [this] { updateScrollControls(); });
    connect(fireRate_, &QSlider::valueChanged, this, &MainWindow::updateFireRateLabel);
    connect(importButton, &QPushButton::clicked, this, &MainWindow::importSettings);
    connect(exportButton, &QPushButton::clicked, this, &MainWindow::exportSettings);
    connect(plan, &QPushButton::clicked, this, &MainWindow::planSettings);
    connect(apply, &QPushButton::clicked, this, &MainWindow::applySettings);
    updateScrollControls();
    updateFireRateLabel(fireRate_->value());
}

void MainWindow::buildMacroPage(QTabWidget *tabs)
{
    auto *page = new QWidget(tabs);
    auto *layout = new QVBoxLayout(page);
    auto *info = new QLabel("Host-side macro JSON. Nothing runs until you explicitly start it.", page);
    info->setWordWrap(true);
    layout->addWidget(info);
    macroEditor_ = new QPlainTextEdit(page);
    macroEditor_->setPlaceholderText("Import or enter a version 1 macro JSON document");
        macroEditor_->setPlainText(R"({
    "version": 1,
    "trigger": { "type": "hotkey", "key": "KeyF", "modifiers": ["ControlLeft", "ShiftLeft"] },
    "steps": [
        { "type": "key", "key": "KeyA", "pressed": true },
        { "type": "delay", "milliseconds": 50 },
        { "type": "key", "key": "KeyA", "pressed": false }
    ]
})");
    macroEditor_->setTabStopDistance(4 * fontMetrics().horizontalAdvance(' '));
    layout->addWidget(macroEditor_, 1);

    auto *options = new QHBoxLayout;
    auto *stopKeyLabel = new QLabel("Stop key", page);
    stopKeyEdit_ = new QLineEdit("F12", page);
    stopKeyEdit_->setMaximumWidth(120);
    options->addWidget(stopKeyLabel);
    options->addWidget(stopKeyEdit_);
    options->addStretch();
    layout->addLayout(options);

    auto *buttons = new QHBoxLayout;
    auto *importButton = new QPushButton("Import JSON...", page);
    auto *exportButton = new QPushButton("Export JSON...", page);
    auto *validate = new QPushButton("Validate", page);
    auto *run = new QPushButton("Run macro", page);
    stopMacroButton_ = new QPushButton("Stop", page);
    stopMacroButton_->setEnabled(false);
    buttons->addWidget(importButton);
    buttons->addWidget(exportButton);
    buttons->addStretch();
    buttons->addWidget(validate);
    buttons->addWidget(run);
    buttons->addWidget(stopMacroButton_);
    layout->addLayout(buttons);
    tabs->addTab(page, "Host macros");

    connect(importButton, &QPushButton::clicked, this, &MainWindow::importMacro);
    connect(exportButton, &QPushButton::clicked, this, &MainWindow::exportMacro);
    connect(validate, &QPushButton::clicked, this, [this] { validateMacro(); });
    connect(run, &QPushButton::clicked, this, &MainWindow::runMacro);
    connect(stopMacroButton_, &QPushButton::clicked, this, &MainWindow::stopMacro);
}

QString MainWindow::cliPath() const
{
    const QString selected = cliPathEdit_->text().trimmed();
    const QFileInfo selectedInfo(selected);
    if (!selected.isEmpty() && selectedInfo.exists() && selectedInfo.isExecutable()) {
        return selectedInfo.absoluteFilePath();
    }
    const QString bundled = QCoreApplication::applicationDirPath() + "/a704f-macos";
    if (QFileInfo(bundled).isExecutable()) {
        return bundled;
    }
    if (selected.isEmpty()) {
        return QStandardPaths::findExecutable("a704f-macos");
    }
    const QString found = QStandardPaths::findExecutable(selected);
    if (!found.isEmpty()) {
        return found;
    }
    if (selectedInfo.exists()) {
        return selectedInfo.absoluteFilePath();
    }
    return selected;
}

QString MainWindow::settingsPath() const
{
    return dataDirectory_ + "/settings.json";
}

QString MainWindow::macroPath() const
{
    return dataDirectory_ + "/active-macro.json";
}

bool MainWindow::writeSettingsFile()
{
    try {
        const QByteArray json = QJsonDocument(settingsObject()).toJson(QJsonDocument::Indented);
        writeJsonAtomically(settingsPath(), json);
        return true;
    } catch (const std::exception &error) {
        QMessageBox::critical(this, "Cannot save settings", QString::fromUtf8(error.what()));
        return false;
    }
}

bool MainWindow::writeMacroFile()
{
    try {
        writeJsonAtomically(macroPath(), macroEditor_->toPlainText().toUtf8());
        return true;
    } catch (const std::exception &error) {
        QMessageBox::critical(this, "Cannot save macro", QString::fromUtf8(error.what()));
        return false;
    }
}

void MainWindow::loadSettingsFile(const QString &path)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        appendOutput(QString("Cannot read settings file: %1").arg(file.errorString()).toUtf8());
        return;
    }
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(file.readAll(), &error);
    if (error.error != QJsonParseError::NoError || !document.isObject()) {
        appendOutput(QString("Invalid settings JSON: %1").arg(error.errorString()).toUtf8());
        return;
    }
    setSettings(document.object());
}

void MainWindow::setSettings(const QJsonObject &settings)
{
    const auto setCombo = [](QComboBox *combo, int value) {
        const int index = combo->findData(value);
        if (index >= 0) {
            combo->setCurrentIndex(index);
        }
    };
    setCombo(rgbMode_, settings.value("rgb_mode").toInt(0x10));
    setCombo(scrollMode_, settings.value("scroll_mode").toInt(0));
    setCombo(scrollDebug_, settings.value("scroll_debug").toInt(0x01));
    fireRate_->setValue(settings.value("fire_rate").toInt(4));
    setCombo(activeDpi_, settings.value("active_dpi").toInt(0));

    const QJsonArray profiles = settings.value("dpi_profiles").toArray();
    for (int index = 0; index < 4 && index < profiles.size(); ++index) {
        const QJsonObject profile = profiles[index].toObject();
        dpiValues_[index]->setValue(profile.value("value").toInt(dpiValues_[index]->value()));
        dpiEnabled_[index]->setChecked(profile.value("enabled").toBool(dpiEnabled_[index]->isChecked()));
    }

    const QJsonArray bindings = settings.value("bindings").toArray();
    for (const QJsonValue &entry : bindings) {
        const QJsonArray pair = entry.toArray();
        if (pair.size() != 2) {
            continue;
        }
        const int buttonCode = pair[0].toInt(-1);
        const auto button = std::find(buttonCodes.begin(), buttonCodes.end(), buttonCode);
        if (button != buttonCodes.end()) {
            setCombo(bindings_[std::distance(buttonCodes.begin(), button)], pair[1].toInt());
        }
    }
    updateScrollControls();
    updateFireRateLabel(fireRate_->value());
}

QJsonObject MainWindow::settingsObject() const
{
    QJsonArray profiles;
    for (int index = 0; index < 4; ++index) {
        profiles.append(QJsonObject{
            {"value", dpiValues_[index]->value()},
            {"enabled", dpiEnabled_[index]->isChecked()}});
    }
    QJsonArray bindings;
    for (int index = 0; index < 7; ++index) {
        bindings.append(QJsonArray{buttonCodes[index], bindings_[index]->currentData().toInt()});
    }
    return {
        {"rgb_mode", rgbMode_->currentData().toInt()},
        {"scroll_mode", scrollMode_->currentData().toInt()},
        {"scroll_debug", scrollMode_->currentData().toInt() == 2 ? scrollDebug_->currentData().toInt() : 0},
        {"fire_rate", fireRate_->value()},
        {"active_dpi", activeDpi_->currentData().toInt()},
        {"dpi_profiles", profiles},
        {"bindings", bindings}};
}

void MainWindow::startCli(const QStringList &arguments, Task task)
{
    if (process_.state() != QProcess::NotRunning) {
        statusLabel_->setText("A CLI task is already running");
        return;
    }
    const QString executable = cliPath();
    if (executable.isEmpty()) {
        QMessageBox::warning(this, "Rust CLI not found", "Build a704f-macos or select its executable above.");
        return;
    }
    task_ = task;
    process_.setProgram(executable);
    process_.setArguments(arguments);
    logOutput_->appendPlainText(QString("> %1 %2").arg(executable, arguments.join(' ')));
    statusLabel_->setText("Working...");
    process_.start();
}

void MainWindow::appendOutput(const QByteArray &output)
{
    const QString text = QString::fromUtf8(output).trimmed();
    if (!text.isEmpty()) {
        if (task_ == Task::Inspect && deviceOutput_) {
            deviceOutput_->appendPlainText(text);
        }
        logOutput_->appendPlainText(text);
    }
}

void MainWindow::updateFireRateLabel(int value)
{
    fireRateLabel_->setText(QString("%1 / 255").arg(value));
}

void MainWindow::updateScrollControls()
{
    scrollDebug_->setEnabled(scrollMode_->currentData().toInt() == 2);
}

void MainWindow::inspectDevice()
{
    deviceOutput_->clear();
    deviceLabel_->setText("Inspecting matching USB HID interfaces...");
    startCli({"device", "inspect"}, Task::Inspect);
}

void MainWindow::planSettings()
{
    if (writeSettingsFile()) {
        startCli({"settings", "plan", "--file", settingsPath()}, Task::Plan);
    }
}

void MainWindow::applySettings()
{
    if (!writeSettingsFile()) {
        return;
    }
    const auto answer = QMessageBox::warning(
        this,
        "Apply settings to hardware?",
        "This sends documented settings to USB 18f8:1286. The mouse provides no documented readback, so acceptance cannot be verified.",
        QMessageBox::Apply | QMessageBox::Cancel,
        QMessageBox::Cancel);
    if (answer == QMessageBox::Apply) {
        startCli({"settings", "apply", "--file", settingsPath(), "--write"}, Task::Apply);
    }
}

void MainWindow::importSettings()
{
    const QString path = QFileDialog::getOpenFileName(this, "Import settings JSON", {}, "JSON files (*.json)");
    if (path.isEmpty()) {
        return;
    }
    QFile source(path);
    if (!source.open(QIODevice::ReadOnly)) {
        QMessageBox::critical(this, "Import failed", source.errorString());
        return;
    }
    try {
        writeJsonAtomically(settingsPath(), source.readAll());
    } catch (const std::exception &error) {
        QMessageBox::critical(this, "Import failed", QString::fromUtf8(error.what()));
        return;
    }
    startCli({"settings", "plan", "--file", settingsPath()}, Task::Plan);
}

void MainWindow::exportSettings()
{
    const QString path = QFileDialog::getSaveFileName(this, "Export settings JSON", "a704f-settings.json", "JSON files (*.json)");
    if (path.isEmpty()) {
        return;
    }
    try {
        writeJsonAtomically(path, QJsonDocument(settingsObject()).toJson(QJsonDocument::Indented));
        statusLabel_->setText("Settings exported");
    } catch (const std::exception &error) {
        QMessageBox::critical(this, "Export failed", QString::fromUtf8(error.what()));
    }
}

void MainWindow::importMacro()
{
    const QString path = QFileDialog::getOpenFileName(this, "Import macro JSON", {}, "JSON files (*.json)");
    if (path.isEmpty()) {
        return;
    }
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        QMessageBox::critical(this, "Import failed", file.errorString());
        return;
    }
    macroEditor_->setPlainText(QString::fromUtf8(file.readAll()));
}

void MainWindow::exportMacro()
{
    const QString path = QFileDialog::getSaveFileName(this, "Export macro JSON", "a704f-macro.json", "JSON files (*.json)");
    if (path.isEmpty()) {
        return;
    }
    try {
        writeJsonAtomically(path, macroEditor_->toPlainText().toUtf8());
        statusLabel_->setText("Macro exported; validate before running");
    } catch (const std::exception &error) {
        QMessageBox::critical(this, "Export failed", QString::fromUtf8(error.what()));
    }
}

void MainWindow::validateMacro(bool runAfterValidation)
{
    if (!writeMacroFile()) {
        return;
    }
    runAfterValidation_ = runAfterValidation;
    startCli({"macro", "validate", "--file", macroPath()}, Task::ValidateMacro);
}

void MainWindow::runMacro()
{
    if (stopKeyEdit_->text().trimmed().isEmpty()) {
        QMessageBox::warning(this, "Invalid stop key", "Enter a stop key such as F12 or Escape.");
        return;
    }
    validateMacro(true);
}

void MainWindow::stopMacro()
{
    if (task_ != Task::RunMacro || process_.state() == QProcess::NotRunning) {
        return;
    }
#if defined(Q_OS_UNIX)
    const auto pid = static_cast<pid_t>(process_.processId());
    if (::kill(pid, SIGINT) != 0) {
        appendOutput("Could not send the stop signal; use the configured global stop key.");
    } else {
        statusLabel_->setText("Stopping macro...");
        stopMacroButton_->setEnabled(false);
    }
#else
    process_.terminate();
#endif
}

void MainWindow::onProcessFinished(int exitCode, QProcess::ExitStatus exitStatus)
{
    appendOutput(process_.readAllStandardOutput());
    appendOutput(process_.readAllStandardError());
    const Task completed = task_;
    task_ = Task::None;
    stopMacroButton_->setEnabled(false);
    const bool success = exitStatus == QProcess::NormalExit && exitCode == 0;

    if (!success) {
        statusLabel_->setText("Command failed; see activity output");
        runAfterValidation_ = false;
        return;
    }

    switch (completed) {
    case Task::Inspect: {
        const QString result = deviceOutput_->toPlainText();
        deviceLabel_->setText(result.contains("18f8:1286")
            ? "A704F interface(s) found. Inspection was read-only."
            : "No supported A704F configuration interface found.");
        statusLabel_->setText("Inspection complete");
        break;
    }
    case Task::Plan:
        statusLabel_->setText("Dry-run plan complete; nothing was written");
        if (QFile::exists(settingsPath())) {
            loadSettingsFile(settingsPath());
        }
        break;
    case Task::Apply:
        statusLabel_->setText("Reports sent; no firmware readback available");
        break;
    case Task::ValidateMacro:
        statusLabel_->setText("Macro JSON is valid");
        if (runAfterValidation_) {
            runAfterValidation_ = false;
            startCli({"macro", "run", "--file", macroPath(), "--stop-key", stopKeyEdit_->text().trimmed()}, Task::RunMacro);
        }
        break;
    case Task::RunMacro:
        statusLabel_->setText("Macro listener stopped");
        break;
    case Task::None:
        break;
    }
}

void MainWindow::onProcessError(QProcess::ProcessError error)
{
    if (error == QProcess::FailedToStart) {
        statusLabel_->setText("Could not start Rust CLI");
        appendOutput(process_.errorString().toUtf8());
        task_ = Task::None;
        runAfterValidation_ = false;
        stopMacroButton_->setEnabled(false);
    }
}

void MainWindow::closeEvent(QCloseEvent *event)
{
    if (task_ == Task::RunMacro && process_.state() != QProcess::NotRunning) {
        stopMacro();
        if (!process_.waitForFinished(3000)) {
            QMessageBox::warning(this, "Macro still running", "The macro listener has not stopped yet. Press the configured stop key and close again.");
            event->ignore();
            return;
        }
    }
    event->accept();
}