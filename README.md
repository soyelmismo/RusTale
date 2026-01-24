# RusTale

**RusTale** is a lightweight, custom launcher and patcher for Hytale, built with performance and flexibility in mind. Written in **Rust**, it offers a modern, responsive user interface and advanced features for game customization and local server management.

![RusTale Banner](https://placehold.co/600x200?text=RusTale+Launcher) *(Replace with actual screenshot)*

## 🚀 Features

### Core Launcher
*   **High Performance**: Built with **Rust** and **Iced** for a snappy, native feel with minimal resource usage.
*   **Cross-Platform Architecture**: Designed to work on Windows (primary) with foundational support for Linux.
*   **Responsive UI**: Modern, fluid interface that adapts to your window size, featuring a polished settings modal and dynamic news section.
*   **System Tray Integration**: Minimize to tray, check updates, and control game execution directly from your taskbar.

### Advanced Patching & Mods
*   **Dynamic DLL Injection**: Includes the **Aurora** subsystem, a custom C-compatible dynamic library (DLL/SO) that performs runtime memory patching (string swapping) to enable custom game behavior without permanent binary modifications.
*   **Mod Management**: Built-in simple mod manager to organize local mods.
*   **ZIP Patching**: Support for applying modifications directly via ZIP archives.
*   **Java Proxy Mode**: Intelligent Java proxy logic to intercept and verify game launch parameters.

### Authentication & Networking
*   **Reactive Issuer Padding**: Smart JWT generation that adapts to the client's `User-Agent`, ensuring compatibility with both official and custom server authentication requirements.
*   **Local Server Emulation**: Integrated local authentication server using `warp` to handle offline play and local development scenarios.
*   **Dynamic Port Selection**: Automatically finds and assigns free ports for local server instances to avoid conflicts.

### Quality of Life
*   **Quickplay Mode**: Instantly launch into the game with your last used profile.
*   **Auto-Updates**: Automatic version checking and self-updating capabilities.
*   **Profile Management**: Create and switch between multiple user profiles with ease.

## 🛠️ Technology Stack

*   **Language**: Rust (2024 Edition)
*   **GUI Framework**: [Iced](https://github.com/iced-rs/iced) (v0.13)
*   **Async Runtime**: Tokio
*   **HTTP Client/Server**: Reqwest & Warp
*   **System Integration**: `tray-icon`, `directories`, `winres`
*   **Patching Engine**: Custom `aurora` crate (cdylib) utilizing raw memory manipulation.

## 📦 structure

*   `launcher/`: Main GUI application and game logic.
*   `aurora/`: Dynamic library for runtime memory patching (injected into the game process).

## ⚠️ Disclaimer & Contact

**This project is created strictly for educational purposes.**

RusTale is an independent project and is not affiliated with, endorsed by, or connected to Hypixel Studios or the Hytale team. All game assets and trademarks belong to their respective owners.

For DMCA concerns or takedown requests, please contact me directly at:
**`el@cocaine.ninja`** (using official mail domain)
