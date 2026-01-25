@echo off
title RusTale Dedicated Server
echo Starting RusTale Server...
echo ----------------------------------------

:: Run rustale directly. 
:: When Ctrl+C is pressed, Rust will catch it thanks to the change in runner.rs
rustale.exe --dedicated-server --online-mode=local --branch=release --game-version=5 --tunnel playit

:: If rustale closes, we get here
echo.
echo Server stopped.
pause
