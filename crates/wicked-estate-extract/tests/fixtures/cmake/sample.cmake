# sample.cmake — integration corpus fixture for the CMake extractor
cmake_minimum_required(VERSION 3.25)

project(
    WickedEstate
    VERSION 0.1.0
    DESCRIPTION "Code-graph parser for LLM agents"
    LANGUAGES CXX C
)

# ---------------------------------------------------------------------------
# Global settings
# ---------------------------------------------------------------------------
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

option(WICKED_ENABLE_TESTS "Build the test suite" ON)
option(WICKED_ENABLE_ASAN  "Enable AddressSanitizer" OFF)

# ---------------------------------------------------------------------------
# find_package dependencies
# ---------------------------------------------------------------------------
find_package(SQLite3 REQUIRED)
find_package(Threads REQUIRED)
find_package(nlohmann_json 3.11 REQUIRED)

# ---------------------------------------------------------------------------
# Helper function: wicked_add_warnings
# ---------------------------------------------------------------------------
function(wicked_add_warnings target)
    target_compile_options(${target} PRIVATE
        $<$<CXX_COMPILER_ID:GNU,Clang,AppleClang>:
            -Wall -Wextra -Wpedantic -Werror>
        $<$<CXX_COMPILER_ID:MSVC>:/W4 /WX>
    )
    if(WICKED_ENABLE_ASAN)
        target_compile_options(${target} PRIVATE -fsanitize=address,undefined)
        target_link_options   (${target} PRIVATE -fsanitize=address,undefined)
    endif()
endfunction()

# ---------------------------------------------------------------------------
# Library: wicked_core
# ---------------------------------------------------------------------------
add_library(wicked_core STATIC
    src/graph.cpp
    src/symbol.cpp
    src/edge.cpp
    src/store/sqlite_store.cpp
)

target_include_directories(wicked_core
    PUBLIC  $<BUILD_INTERFACE:${PROJECT_SOURCE_DIR}/include>
    PRIVATE ${PROJECT_SOURCE_DIR}/src
)

target_link_libraries(wicked_core
    PUBLIC  SQLite::SQLite3 nlohmann_json::nlohmann_json
    PRIVATE Threads::Threads
)

wicked_add_warnings(wicked_core)

# ---------------------------------------------------------------------------
# Executable: wicked_estate
# ---------------------------------------------------------------------------
add_executable(wicked_estate src/main.cpp)

target_link_libraries(wicked_estate PRIVATE wicked_core)
wicked_add_warnings(wicked_estate)

install(TARGETS wicked_estate DESTINATION bin)

# ---------------------------------------------------------------------------
# Tests (optional)
# ---------------------------------------------------------------------------
if(WICKED_ENABLE_TESTS)
    enable_testing()
    find_package(GTest REQUIRED)

    add_executable(wicked_tests
        tests/test_graph.cpp
        tests/test_symbol.cpp
        tests/test_sqlite_store.cpp
    )
    target_link_libraries(wicked_tests PRIVATE wicked_core GTest::gtest_main)
    wicked_add_warnings(wicked_tests)

    include(GoogleTest)
    gtest_discover_tests(wicked_tests)
endif()
