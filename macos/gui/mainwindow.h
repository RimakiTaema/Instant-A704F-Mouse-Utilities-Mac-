#ifndef A704F_MAINWINDOW_H
#define A704F_MAINWINDOW_H

#include <QMainWindow>
#include <QProcess>

#include <array>

class QCheckBox;
class QCloseEvent;
class QComboBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QSlider;
class QSpinBox;

class MainWindow final : public QMainWindow
{
    Q_OBJECT

public:
    explicit MainWindow(QWidget *parent = nullptr);

protected:
    void closeEvent(QCloseEvent *event) override;

private:
    enum class Task {
        None,
        Inspect,
        Plan,
        Apply,
        ValidateMacro,
        RunMacro
    };

    void buildUi();
    void buildDevicePage(class QTabWidget *tabs);
    void buildSettingsPage(class QTabWidget *tabs);
    void buildMacroPage(class QTabWidget *tabs);
    QString cliPath() const;
    QString settingsPath() const;
    QString macroPath() const;
    bool writeSettingsFile();
    bool writeMacroFile();
    void loadSettingsFile(const QString &path);
    void setSettings(const class QJsonObject &settings);
    class QJsonObject settingsObject() const;
    void startCli(const QStringList &arguments, Task task);
    void appendOutput(const QByteArray &output);
    void updateFireRateLabel(int value);
    void updateScrollControls();
    void inspectDevice();
    void planSettings();
    void applySettings();
    void importSettings();
    void exportSettings();
    void importMacro();
    void exportMacro();
    void validateMacro(bool runAfterValidation = false);
    void runMacro();
    void stopMacro();
    void onProcessFinished(int exitCode, QProcess::ExitStatus exitStatus);
    void onProcessError(QProcess::ProcessError error);

    QProcess process_;
    Task task_ = Task::None;
    bool runAfterValidation_ = false;
    QString dataDirectory_;

    QLineEdit *cliPathEdit_ = nullptr;
    QLabel *statusLabel_ = nullptr;
    QLabel *deviceLabel_ = nullptr;
    QPlainTextEdit *deviceOutput_ = nullptr;
    QPlainTextEdit *logOutput_ = nullptr;
    QPlainTextEdit *macroEditor_ = nullptr;
    QComboBox *rgbMode_ = nullptr;
    QComboBox *scrollMode_ = nullptr;
    QComboBox *scrollDebug_ = nullptr;
    QSlider *fireRate_ = nullptr;
    QLabel *fireRateLabel_ = nullptr;
    QComboBox *activeDpi_ = nullptr;
    std::array<QSpinBox *, 4> dpiValues_{};
    std::array<QCheckBox *, 4> dpiEnabled_{};
    std::array<QComboBox *, 7> bindings_{};
    QLineEdit *stopKeyEdit_ = nullptr;
    QPushButton *stopMacroButton_ = nullptr;
};

#endif