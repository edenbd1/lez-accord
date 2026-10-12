cmake_minimum_required(VERSION 3.20)
project(AccordPlugin LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(DEFINED ENV{LOGOS_MODULE_BUILDER_ROOT})
    include($ENV{LOGOS_MODULE_BUILDER_ROOT}/cmake/LogosModule.cmake)
else()
    message(FATAL_ERROR "LogosModule.cmake not found. Build with `nix build .#lgx-portable`; "
                        "the reader alone builds with plain Qt: see tests/CMakeLists.txt.")
endif()

file(READ "${CMAKE_CURRENT_SOURCE_DIR}/metadata.json" METADATA_JSON)
string(JSON MODULE_NAME GET ${METADATA_JSON} name)
string(JSON MODULE_VERSION GET ${METADATA_JSON} version)

# A ui_qml module: the QML view runs in Basecamp, this backend in its own
# process, and the two talk over Qt Remote Objects through src/accord.rep.
find_package(Qt6 REQUIRED COMPONENTS Network)

logos_module(
    NAME ${MODULE_NAME}
    REP_FILE src/accord.rep
    SOURCES
        src/accord_backend.h
        src/accord_backend.cpp
        src/accord_chain.h
        src/accord_chain.cpp
    INCLUDE_DIRS
        src
)

target_link_libraries(${MODULE_NAME}_module_plugin PRIVATE Qt6::Network)
target_compile_definitions(${MODULE_NAME}_module_plugin PRIVATE ACCORD_VERSION="${MODULE_VERSION}")
