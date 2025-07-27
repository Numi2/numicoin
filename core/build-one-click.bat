@echo off
echo 🏗️  Building NumiCoin One-Click Miner...
echo =========================================

REM Clean previous builds
echo 🧹 Cleaning previous builds...
cargo clean

REM Build with maximum optimizations for distribution
echo ⚡ Building optimized release binary...
cargo build --release --bin numi-one-click

REM Check if build was successful
if not exist "target\release\numi-one-click.exe" (
    echo ❌ Build failed - executable not found
    exit /b 1
)

REM Create distribution directory
set DIST_DIR=dist
if not exist "%DIST_DIR%" mkdir "%DIST_DIR%"

REM Copy executable with a user-friendly name
copy "target\release\numi-one-click.exe" "%DIST_DIR%\NumiCoin-Miner.exe"
echo ✅ Windows executable created: %DIST_DIR%\NumiCoin-Miner.exe

REM Create README for distribution
echo NumiCoin One-Click Miner > "%DIST_DIR%\README.txt"
echo ======================== >> "%DIST_DIR%\README.txt"
echo. >> "%DIST_DIR%\README.txt"
echo This is a simple, one-click cryptocurrency miner for NumiCoin. >> "%DIST_DIR%\README.txt"
echo. >> "%DIST_DIR%\README.txt"
echo HOW TO USE: >> "%DIST_DIR%\README.txt"
echo 1. Double-click NumiCoin-Miner.exe to start mining >> "%DIST_DIR%\README.txt"
echo 2. A wallet will be automatically created for you >> "%DIST_DIR%\README.txt"
echo 3. Mining will begin immediately >> "%DIST_DIR%\README.txt"
echo 4. Press Ctrl+C to stop and exit >> "%DIST_DIR%\README.txt"
echo. >> "%DIST_DIR%\README.txt"
echo WHAT IT DOES: >> "%DIST_DIR%\README.txt"
echo - Creates your personal wallet automatically >> "%DIST_DIR%\README.txt"
echo - Starts mining NumiCoin with your CPU >> "%DIST_DIR%\README.txt"
echo - Shows real-time mining progress >> "%DIST_DIR%\README.txt"
echo - Saves all data in the same folder >> "%DIST_DIR%\README.txt"
echo. >> "%DIST_DIR%\README.txt"
echo FILES CREATED: >> "%DIST_DIR%\README.txt"
echo - my-wallet.json: Your wallet (KEEP THIS SAFE!) >> "%DIST_DIR%\README.txt"
echo - numi-data\: Blockchain data directory >> "%DIST_DIR%\README.txt"
echo. >> "%DIST_DIR%\README.txt"
echo SECURITY NOTE: >> "%DIST_DIR%\README.txt"
echo Keep your wallet file safe! It contains your private keys. >> "%DIST_DIR%\README.txt"
echo Back it up somewhere secure. >> "%DIST_DIR%\README.txt"

echo.
echo 🎉 Build complete!
echo 📁 Distribution files in: %DIST_DIR%\
echo 🚀 Users can now simply download and run NumiCoin-Miner.exe!
echo.
echo Next steps:
echo 1. Test the executable: %DIST_DIR%\NumiCoin-Miner.exe
echo 2. Distribute the files in the %DIST_DIR%\ folder
echo 3. Users just need to double-click to start mining!

pause 