#!/usr/bin/env sh
set -eu

add_to_path=false
prompt_for_path=true

for argument in "$@"; do
    case "$argument" in
        --add-to-path)
            add_to_path=true
            ;;
        --no-path-prompt)
            prompt_for_path=false
            ;;
        *)
            printf 'Unknown option: %s\nUsage: %s [--add-to-path] [--no-path-prompt]\n' "$argument" "$0" >&2
            exit 2
            ;;
    esac
done

project_directory=$(CDPATH= cd "$(dirname "$0")" && pwd)
install_directory=${XDG_BIN_HOME:-"$HOME/.local/bin"}
source_executable="$project_directory/target/release/picopilot"
installed_executable="$install_directory/picopilot"

if ! command -v cargo >/dev/null 2>&1; then
    printf '%s\n' 'Cargo was not found. Install Rust from https://rustup.rs/ and reopen this terminal.' >&2
    exit 1
fi

printf '%s\n' 'Building picopilot in release mode...'
(cd "$project_directory" && cargo build --release --locked)

mkdir -p "$install_directory"
cp "$source_executable" "$installed_executable"
chmod 755 "$installed_executable"
printf 'Installed picopilot to %s\n' "$installed_executable"

case ":$PATH:" in
    *":$install_directory:"*)
        printf '%s\n' 'The install directory is already on PATH.'
        exit 0
        ;;
esac

if [ "$add_to_path" = false ] && [ "$prompt_for_path" = true ] && [ -t 0 ]; then
    printf 'Add %s to your PATH? [Y/n] ' "$install_directory"
    read -r answer
    case "$answer" in
        ''|[Yy]|[Yy][Ee][Ss]) add_to_path=true ;;
    esac
fi

if [ "$add_to_path" = true ]; then
    shell_name=$(basename "${SHELL:-sh}")
    case "$shell_name" in
        zsh) profile_file="$HOME/.zshrc" ;;
        bash) profile_file="$HOME/.bashrc" ;;
        *) profile_file="$HOME/.profile" ;;
    esac

    path_line='export PATH="${XDG_BIN_HOME:-$HOME/.local/bin}:$PATH"'
    if [ ! -f "$profile_file" ] || ! grep -Fqx "$path_line" "$profile_file"; then
        printf '\n%s\n' "$path_line" >> "$profile_file"
    fi
    printf 'Added the install directory to PATH in %s. Open a new terminal to use picopilot everywhere.\n' "$profile_file"
else
    printf 'PATH was not changed. Run picopilot from %s\n' "$installed_executable"
fi