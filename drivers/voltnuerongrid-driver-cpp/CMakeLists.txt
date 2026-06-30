cmake_minimum_required(VERSION 3.15)
project(voltnuerongrid_driver_cpp LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Header-only interface library.
add_library(voltnuerongrid_cpp INTERFACE)
target_include_directories(voltnuerongrid_cpp INTERFACE
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/../voltnuerongrid-driver-c) # for voltnuerongrid.h

# Location of the built C cdylib (override with -DVNG_C_LIB_DIR=...).
set(VNG_C_LIB_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../target/debug"
    CACHE PATH "Directory containing libvoltnuerongrid_driver")

# Offline self-contained smoke test (stubs the C ABI; no link to cdylib).
add_executable(vng_cpp_smoke tests/smoke.cpp)
target_link_libraries(vng_cpp_smoke PRIVATE voltnuerongrid_cpp)

enable_testing()
add_test(NAME vng_cpp_smoke COMMAND vng_cpp_smoke)

# Live example linking against the real C cdylib (run against a server).
add_executable(vng_cpp_sample examples/sample.cpp)
target_link_libraries(vng_cpp_sample PRIVATE voltnuerongrid_cpp)
target_link_directories(vng_cpp_sample PRIVATE ${VNG_C_LIB_DIR})
target_link_libraries(vng_cpp_sample PRIVATE voltnuerongrid_driver)
