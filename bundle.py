from io import BytesIO
import json
import os
import subprocess
import argparse
import shutil
import requests
from sys import exit
from pathlib import Path
from typing import Any, TypedDict, Unpack
from urllib.request import urlopen
from zipfile import ZipFile
from dataclasses import dataclass

script_dir = Path(__file__).resolve().parent


class BuildRustBinaryParams(TypedDict):
    gtk_bin_dir: str


def build_rust_binary(**kwargs: Unpack[BuildRustBinaryParams]):
    """
    Build the GTK application using `cargo build`

    Returns the path to the built binary.
    """

    # Ensure GTK is available to gtk-rs:
    gtk_bin_dir = Path(kwargs["gtk_bin_dir"])
    my_env = os.environ.copy()
    my_env["PKG_CONFIG_PATH"] = str(gtk_bin_dir.joinpath("../lib/pkgconfig").resolve())
    my_env["PATH"] = my_env["PATH"] + ";" + str(gtk_bin_dir.resolve())
    my_env["LIB"] = my_env["LIB"] + ";" + str(gtk_bin_dir.joinpath("../lib").resolve())

    # Start build:
    try:
        result = subprocess.run(
            ["cargo", "build", "--release", "--message-format", "json"],
            cwd=script_dir,
            stdout=subprocess.PIPE,
            stdin=subprocess.DEVNULL,
            check=True,
            env=my_env,
        )
    except:
        print("Re-running cargo build without capturing stdout to see all errors:\n")
        subprocess.run(
            ["cargo", "build", "--release"],
            cwd=script_dir,
            stdin=subprocess.DEVNULL,
            env=my_env,
        )
        raise

    # Find built binaries:
    text = result.stdout.decode("utf-8")

    binaries: list[str] = []
    for line in text.splitlines():
        msg = json.loads(line)
        if msg["reason"] != "compiler-artifact":
            continue
        if not msg["executable"] or not isinstance(msg["executable"], str):
            continue
        binaries.append(msg["executable"])

    assert len(binaries) == 1

    return binaries[0]


@dataclass
class CargoMetadata:
    target_directory: str
    package_name: str


def get_cargo_metadata() -> CargoMetadata:
    """Get the path where Cargo writes its build artifacts."""
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=script_dir,
        stdout=subprocess.PIPE,
        stdin=subprocess.DEVNULL,
        check=True,
    )
    text = result.stdout.decode("utf-8")
    metadata = json.loads(text)

    if metadata["target_directory"] is None:
        raise ValueError(
            'The returned json object didn\'t define a "target_directory" property'
        )
    if not isinstance(metadata["target_directory"], str):
        raise ValueError("target_directory is not a string")

    target_dir = metadata["target_directory"]

    if not metadata["packages"]:
        raise ValueError(
            'The returned json object didn\'t define a "packages" property'
        )
    if not isinstance(metadata["packages"], list):
        raise ValueError("packages is not a list")
    if len(metadata["packages"]) != 1:
        raise ValueError("packages is not a list of length 1")
    if metadata["packages"][0]["name"] is None:
        raise ValueError('The package didn\'t define a "name" property')
    if not isinstance(metadata["packages"][0]["name"], str):
        raise ValueError("name is not a string")

    package_name = metadata["packages"][0]["name"]
    return CargoMetadata(target_directory=target_dir, package_name=package_name)


def show_file_in_explorer(file: str):
    """Show the given file in the Windows Explorer."""
    # Can't end with a trailing path separator:
    if file.endswith("\\") or file.endswith("/"):
        file = file[:-1]
    # Note: The comma after select is not a typo
    subprocess.run(["explorer", "/select,", file])


def bundle():
    """
    Package the GTK application.

    This ensures all runtime dependencies are available when running on Windows.
    """
    parser = argparse.ArgumentParser(
        description="Package the GTK application for Windows."
    )
    parser.add_argument(
        "--show-binary",
        action="store_true",
        help="Open the bundle directory in Windows Explorer and highlight the GTK application",
    )
    parser.add_argument(
        "--show-zip",
        action="store_true",
        help="Open Windows file explorer and highlight the zip file",
    )
    parser.add_argument(
        "--download-gtk",
        action="store_true",
        help="Download and use latest GTK release by gvsbuild from https://github.com/wingtk/gvsbuild/releases \n\nOtherwise we assume GTK has been built from source using gvsbuild.",
    )
    args = parser.parse_args()

    print()
    cargo_meta = get_cargo_metadata()

    gtk_bin_dir: Path
    if args.download_gtk:
        print(
            "Downloading latest GTK release by gvsbuild from https://github.com/wingtk/gvsbuild/releases"
        )
        print()
        gtk_bin_dir = Path(cargo_meta.target_directory).joinpath("./gtk-from-github")

        if gtk_bin_dir.exists():
            print("GTK release already downloaded at: " + str(gtk_bin_dir))
            print()
        else:
            # Find download URL for latest GitHub release:
            response = requests.get(
                "https://api.github.com/repos/wingtk/gvsbuild/releases/latest"
            )
            if response.status_code != 200:
                print(
                    "Could not download GTK release, HTTP status code: "
                    + str(response.status_code)
                )
                print()
                exit(1)
            data = response.json()
            # print(json.dumps(data, indent=4)) # <- pretty print JSON response
            if data["assets"] is None or len(data["assets"]) == 0:
                print("Could not download GTK release, no assets found")
                print()
                exit(1)
            gtk_assets: Any = list(
                filter(
                    lambda asset: "GTK4" in str(asset["name"]),  # type: ignore
                    data["assets"],
                )  # type: ignore
            )
            if len(gtk_assets) != 1:
                print(
                    "Could not download GTK release, no assets with the name GTK4 was found"
                )
                print("Found assets:")
                print(json.dumps(data["assets"], indent=4))
                print()
                exit(1)
            gtk_asset = gtk_assets[0]
            print(
                "Downloading amd unzipping GitHub release asset: "
                + str(gtk_asset["name"])
            )

            if gtk_asset["browser_download_url"] is None:
                print("Could not download GTK release, no browser_download_url found")
                print()
                exit(1)
            if gtk_asset["content_type"] != "application/zip":
                print(
                    "Could not download GTK release, content_type is not application/zip, instead it was: "
                    + str(gtk_asset["content_type"])
                )
                print()
                exit(1)
            gtk_url = gtk_asset["browser_download_url"]
            print("\tFrom URL: " + str(gtk_url))
            print("\tTo folder at: " + str(gtk_bin_dir))

            # Create output folder:
            gtk_bin_dir.mkdir(parents=True, exist_ok=True)

            # https://stackoverflow.com/questions/64990197/download-and-extract-zip-file
            with urlopen(gtk_url) as zip_response:
                with ZipFile(BytesIO(zip_response.read())) as zip_file:
                    zip_file.extractall(gtk_bin_dir)
            print()

        gtk_bin_dir = gtk_bin_dir.joinpath("bin")
    else:
        # TODO: we should allow customizing where GTK is located.
        gtk_bin_dir = Path("C:/gtk-build/gtk/x64/release/bin")

    if not gtk_bin_dir.exists():
        print("Could not find GTK bin folder at: " + str(gtk_bin_dir))
        print()
        print("- To build GTK from source follow the instructions at")
        print("  https://github.com/wingtk/gvsbuild?tab=readme-ov-file#build-gtk")
        print()
        print(
            "- Alternatively specify the --download-gtk flag to automatically download"
        )
        print(
            "  the latest GTK release by gvsbuild from https://github.com/wingtk/gvsbuild/releases"
        )
        print()
        exit(1)

    print("Found GTK bin folder at: " + str(gtk_bin_dir))
    print()

    # Create the bundle directory
    bundle_dir = Path(cargo_meta.target_directory).joinpath(
        "./release/" + cargo_meta.package_name + "-bundled"
    )
    print("Creating bundle directory at: " + str(bundle_dir))
    bundle_dir.mkdir(parents=True, exist_ok=True)
    print()

    # Copy DLL files
    print("Copying DLL files...")
    for dll in gtk_bin_dir.glob("*.dll"):
        print("\t" + str(dll))
        shutil.copy2(dll, bundle_dir)
    print()

    # Copy ancillary binaries, see: https://discourse.gnome.org/t/gtk-warning-about-gdbus-exe-not-being-found-on-windows-msys2/2893/4
    print("Copying ancillary binaries...")
    gdbus_exe = gtk_bin_dir.joinpath("gdbus.exe")
    print("\t" + str(gdbus_exe))
    shutil.copy2(gdbus_exe, bundle_dir)
    print()

    # Copy "glib compiled schemas", see: https://www.gtk.org/docs/installations/windows
    print("Copying glib compiled schemas...")
    glib_compiled_schemas = gtk_bin_dir.joinpath(
        "../share/glib-2.0/schemas/gschemas.compiled"
    ).resolve()
    schema_out_dir = bundle_dir.joinpath("./share/glib-2.0/schemas")
    schema_out_dir.mkdir(parents=True, exist_ok=True)
    print("\t" + str(glib_compiled_schemas))
    shutil.copy2(glib_compiled_schemas, schema_out_dir)
    print()

    # Specify font (doesn't affect the app for now...):
    print('Creating "share/gtk-4.0/settings.ini" to specify font')
    settings_ini = bundle_dir.joinpath("./share/gtk-4.0/settings.ini")
    print("\tAt: " + str(settings_ini))
    settings_ini.parent.mkdir(parents=True, exist_ok=True)
    with settings_ini.open("w") as f:
        f.write("[Settings]\n")
        f.write("gtk-font-name=Segoe UI 10\n")
    print()

    # According to the instructions at https://www.gtk.org/docs/installations/windows we need to also do:
    # TODO: download fallback icons for Adwaita at https://download.gnome.org/sources/adwaita-icon-theme/
    # TODO: download fallback icons for hicolor at https://www.freedesktop.org/wiki/Software/icon-theme/
    # - hicolor icon issue is mentioned in the GTK-RS book: https://gtk-rs.org/gtk4-rs/stable/latest/book/libadwaita.html#work-around-missing-icons
    #   - Links to: https://gitlab.gnome.org/GNOME/gtk/-/blob/34b9ec5be2f3a38e1e72c4d96f130a2b14734121/NEWS#L60
    #   - Links to: https://gitlab.gnome.org/GNOME/gtk/-/issues/5303

    # Build the GTK application
    print("Building GTK application...")
    print()
    rust_bin = build_rust_binary(gtk_bin_dir=str(gtk_bin_dir))
    print()

    # Copy the GTK application
    rust_bin_to = bundle_dir.joinpath(Path(rust_bin).name)
    print("Copying binary to bundle directory")
    print("\tFrom: " + str(rust_bin))
    print("\tTo:   " + str(rust_bin_to))
    shutil.copy2(rust_bin, rust_bin_to)
    print()

    # Show the bundle in the Windows Explorer
    if args.show_binary:
        print(
            "Showing app inside bundle folder in Windows Explorer (because of --show-binary flag)"
        )
        show_file_in_explorer(str(rust_bin_to))
        print()

    # Create a zip file
    print("Zipping bundle directory to: " + str(bundle_dir.with_suffix(".zip")))
    shutil.make_archive(str(bundle_dir), "zip", bundle_dir)
    print()

    # Show the zip file in the Windows Explorer
    if args.show_zip:
        print("Showing zip file in Windows Explorer (because of --show-zip flag)")
        show_file_in_explorer(str(bundle_dir.with_suffix(".zip")))
        print()

    print("Done!")
    print()


if __name__ == "__main__":
    bundle()
