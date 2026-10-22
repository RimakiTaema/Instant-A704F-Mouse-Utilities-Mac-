#include "mainwindow.h"

#include <QApplication>

int main(int argc, char *argv[])
{
    QApplication app(argc, argv);
    QCoreApplication::setOrganizationName("AmberFrost");
    QCoreApplication::setApplicationName("A704F Configurator");

    MainWindow window;
    window.show();
    return app.exec();
}