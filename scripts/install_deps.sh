#!/bin/bash

# Detect OS and install dependencies
if [ -f /etc/os-release ]; then
    . /etc/os-release
    OS=$NAME
elif type lsb_release >/dev/null 2>&1; then
    OS=$(lsb_release -si)
else
    OS=$(uname -s)
fi

echo "Detected OS: $OS"

if [[ "$OS" == *"Ubuntu"* ]] || [[ "$OS" == *"Debian"* ]] || [[ "$OS" == *"Pop"* ]] || [[ "$OS" == *"Mint"* ]] || [[ "$OS" == *"Kali"* ]]; then
    echo "Installing dependencies for Debian/Ubuntu-based system..."
    sudo apt-get update
    sudo apt-get install -y libxdo-dev
    
elif [[ "$OS" == *"Fedora"* ]] || [[ "$OS" == *"CentOS"* ]] || [[ "$OS" == *"Red Hat"* ]]; then
    echo "Installing dependencies for RHEL/Fedora-based system..."
    # Intenta usar dnf, si no está disponible usa yum
    if command -v dnf &> /dev/null; then
        sudo dnf install -y libxdo-devel
    else
        sudo yum install -y libxdo-devel
    fi

elif [[ "$OS" == *"Arch"* ]] || [[ "$OS" == *"Manjaro"* ]] || [[ "$OS" == *"EndeavourOS"* ]]; then
    echo "Installing dependencies for Arch-based system..."
    sudo pacman -Sy --noconfirm xdotool
    
elif [[ "$OS" == *"Alpine"* ]]; then
    echo "Installing dependencies for Alpine Linux..."
    sudo apk add xdotool-dev

elif [[ "$OS" == *"Suse"* ]] || [[ "$OS" == *"openSUSE"* ]]; then
    echo "Installing dependencies for OpenSUSE..."
    sudo zypper install -y libxdo-devel

else
    echo "------------------------------------------------------------------------"
    echo "WARNING: Could not detect a supported distribution automatically."
    echo "Detected: $OS"
    echo "Please manually install the equivalent of 'libxdo-dev' (libxdo development headers/libraries)."
    echo "------------------------------------------------------------------------"
    exit 1
fi

echo "Dependencies check/installation finished."
