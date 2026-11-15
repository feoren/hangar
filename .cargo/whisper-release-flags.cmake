# Included into whisper.cpp's CMake projects through CMAKE_PROJECT_INCLUDE (see config.toml).
#
# The cmake crate (0.1.58) replaces CMAKE_<LANG>_FLAGS_RELEASE when it drives the Visual Studio
# generator for an MSVC target and drops every /O flag from it, so whisper.cpp would build
# without optimization: measured about 50 times slower than an optimized build. Restore MSVC's
# own release flags. Other compilers keep what CMake chose.
if(MSVC)
  string(APPEND CMAKE_C_FLAGS_RELEASE " /O2 /Ob2 /DNDEBUG")
  string(APPEND CMAKE_CXX_FLAGS_RELEASE " /O2 /Ob2 /DNDEBUG")
endif()
