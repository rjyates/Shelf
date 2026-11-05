cmake_minimum_required(VERSION 3.22.1)
project(shelf_ebc)

add_library(shelf_ebc SHARED shelf_ebc.cpp)
find_library(log-lib log)
target_link_libraries(shelf_ebc ${log-lib})
