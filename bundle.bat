@ECHO OFF
ECHO.This script will prepare for distributing a GTK application to Windows.
ECHO.
ECHO.Change current directory to this script's directory: "%~dp0"
cd /D "%~dp0"

ECHO.Creating "target/gtk-bundle" folder and changing directory to it.
mkdir target/gtk-bundle
pushd "./target/gtk-bundle"

ECHO.Copy all .dll files:
COPY "C:\gtk-build\gtk\x64\release\bin\*.dll" "./"

ECHO.Copy "gdbus.exe" since the program will start that if it is available:
COPY "C:\gtk-build\gtk\x64\release\bin\gdbus.exe" "./"

ECHO.Copy "share/glib-2.0/schemas", see: https://www.gtk.org/docs/installations/windows
mkdir ".\share\glib-2.0\schemas"
COPY "C:\gtk-build\gtk\x64\release\share\glib-2.0\schemas\gschemas.compiled" ".\share\glib-2.0\schemas\gschemas.compiled" 

ECHO.TODO: download fallback icons for Adwaita at https://download.gnome.org/sources/adwaita-icon-theme/ and for hicolor at https://www.freedesktop.org/wiki/Software/icon-theme/

popd

ECHO.Pausing for 5 seconds so we can see errors:
pause 5