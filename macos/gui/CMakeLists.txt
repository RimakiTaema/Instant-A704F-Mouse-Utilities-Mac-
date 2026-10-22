cmake_minimum_required(VERSION 3.21)

project(A704FConfigurator VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.2 REQUIRED COMPONENTS Widgets)

set(A704F_CLI_EXECUTABLE
    "${CMAKE_CURRENT_SOURCE_DIR}/../target/release/a704f-macos"
    CACHE FILEPATH "Path to the a704f-macos CLI executable"
)

qt_add_executable(a704f-configurator
    main.cpp
    mainwindow.cpp
    mainwindow.h
)

target_link_libraries(a704f-configurator PRIVATE Qt6::Widgets)
target_compile_definitions(a704f-configurator PRIVATE
    A704F_CLI_DEFAULT_PATH=\"${A704F_CLI_EXECUTABLE}\"
)

if(APPLE)
    set_target_properties(a704f-configurator PROPERTIES
        MACOSX_BUNDLE TRUE
        MACOSX_BUNDLE_GUI_IDENTIFIER com.amberfrost.a704fconfigurator
        MACOSX_BUNDLE_BUNDLE_VERSION ${PROJECT_VERSION}
        MACOSX_BUNDLE_SHORT_VERSION_STRING ${PROJECT_VERSION}
    )
    if(EXISTS "${A704F_CLI_EXECUTABLE}")
        add_custom_command(TARGET a704f-configurator POST_BUILD
            COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "${A704F_CLI_EXECUTABLE}"
                "$<TARGET_BUNDLE_DIR:a704f-configurator>/Contents/MacOS/a704f-macos"
            VERBATIM
        )
    else()
        message(WARNING "Rust CLI not found at ${A704F_CLI_EXECUTABLE}; build it first or select it in the GUI.")
    endif()
endif()

install(TARGETS a704f-configurator
    BUNDLE DESTINATION .
    RUNTIME DESTINATION bin
)