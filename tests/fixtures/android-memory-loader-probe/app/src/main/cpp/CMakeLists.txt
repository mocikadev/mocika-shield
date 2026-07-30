cmake_minimum_required(VERSION 3.22.1)
project(memoryprobe)

add_library(memoryprobe SHARED memoryprobe.cpp)
target_link_options(memoryprobe PRIVATE
    "-Wl,--hash-style=both"
    "-Wl,-z,max-page-size=16384")
