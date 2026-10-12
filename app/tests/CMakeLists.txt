# The reader on its own (Qt Core, Network) and the real view outside Basecamp (Qt Quick).
#   cmake -S app/tests -B build/app -DCMAKE_PREFIX_PATH=<qt> && cmake --build build/app
#   build/app/chain_test app/tests/fixtures
cmake_minimum_required(VERSION 3.20)
project(accord_app_tests LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
find_package(Qt6 REQUIRED COMPONENTS Core Network)
add_executable(chain_test chain_test.cpp ../src/accord_chain.cpp ../src/accord_chain.h)
target_include_directories(chain_test PRIVATE ../src)
target_link_libraries(chain_test PRIVATE Qt6::Core Qt6::Network)
find_package(Qt6 COMPONENTS Gui Quick Qml)
if(Qt6Quick_FOUND)
    add_executable(qml_host qml_host.cpp ../src/accord_chain.cpp ../src/accord_chain.h)
    target_include_directories(qml_host PRIVATE ../src)
    target_link_libraries(qml_host PRIVATE Qt6::Core Qt6::Network Qt6::Gui Qt6::Quick Qt6::Qml)
endif()
