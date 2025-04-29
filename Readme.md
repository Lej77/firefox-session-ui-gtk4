# firefox-session-ui-gtk4

This is a graphical user interface for interacting with Firefox's session store
file that contains info about currently opened tabs and windows.

## Build

To build this program you need to have `GTK4` installed, for instructions see
[Windows - GUI development with Rust and GTK
4](https://gtk-rs.org/gtk4-rs/stable/latest/book/installation_windows.html). In
short you need to install `gvsbuild` using `python` and then use that to build
`GTK4`.

Alternatively you can download the latest GitHub release from
<https://github.com/wingtk/gvsbuild/releases> though the `gvsbuild` readme
recommends against doing that. (The `bundle.py` script can do this automatically
if passed the `--download-gtk` flag.)

```powershell
# Install Chocolatey package manager:
Set-ExecutionPolicy Bypass -Scope Process -Force; iex ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))

# Install Git:
choco install git

# MSYS2 is a collection of tools and libraries providing you with an easy-to-use environment for building:
choco install msys2

# Install Visual Studio 2022
choco install visualstudio2022-workload-vctools

# Manually install Python: https://www.python.org/downloads/windows/

# Install gvsbuild:
py -3.12 -m pip install --user pipx
py -3.12 -m pipx ensurepath
pipx install gvsbuild

# If you already had an old version, then to upgrade:
pipx install gvsbuild --upgrade
# OR:
pipx uninstall gvsbuild
pipx install gvsbuild

# Build GTK:
gvsbuild build gtk4

# Setup environment variables (automated by VS Code config):
$env:PKG_CONFIG_PATH =  "C:\gtk-build\gtk\x64\release\lib\pkgconfig"
$env:PATH = $env:PATH + ";C:\gtk-build\gtk\x64\release\bin"
$env:LIB  = $env:LIB  + ";C:\gtk-build\gtk\x64\release\lib"

# Now you can build this program:
cargo build --release

# Or when developing:
cargo run
```

## Package for other computers

If you want to run the built program on another computer, then make sure to bring the required `.dll` files:

- <https://www.gtk.org/docs/installations/windows#building-and-distributing-your-application>
- <https://stackoverflow.com/questions/49092784/how-to-distribute-a-gtk-application-on-windows>
- This is the Windows package script for a Rust program that uses `GTK4`: [czkawka/.github/workflows/windows.yml at 2a32a52aa882f6ff52ba4d3e24a666dc2a86cf9b · qarmin/czkawka](https://github.com/qarmin/czkawka/blob/2a32a52aa882f6ff52ba4d3e24a666dc2a86cf9b/.github/workflows/windows.yml#L141-L162)
- Info about how a Rust program was packaged with GTK for different platforms: [Czkawka 6.1.0 - advanced duplicate finder, now with faster caching, exporting results to json, faster short scannings, added logging, improved cli : r/rust](https://www.reddit.com/r/rust/comments/178b6a3/czkawka_610_advanced_duplicate_finder_now_with/)

The `bundle.py` and `bundle.bat` files in this repository attempts to do this and so it should be enough to run one of those:

```powershell
python bundle.py --show-zip
```

## License

This project is released under either:

- [MIT License](./LICENSE-MIT)
- [Apache License (Version 2.0)](./LICENSE-APACHE)

at your choosing.

Note that some optional dependencies might be under different licenses.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
