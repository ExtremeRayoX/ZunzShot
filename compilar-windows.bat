@echo off
setlocal enabledelayedexpansion
chcp 65001 >nul
cd /d "%~dp0"

title Compilar ZunzShot v1.0.2 para Windows

echo ========================================================
echo   ZunzShot v1.0.2 - Compilador Automatico para Windows
echo ========================================================
echo.

:: 1. Asegurar cargo en el PATH de esta sesion
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

:: 2. Verificar Node.js y npm
where npm >nul 2>nul
if errorlevel 1 goto :no_npm

:: 3. Verificar Rust / Cargo
where cargo >nul 2>nul
if errorlevel 1 goto :no_cargo

:: 4. Comprobar Visual Studio Build Tools C++
set "HAS_MSVC=0"
where cl >nul 2>nul && set "HAS_MSVC=1"
if "!HAS_MSVC!"=="1" goto :check_msvc_done

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" goto :check_msvc_warn
for /f "usebackq tokens=*" %%i in (`""%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath"`) do (
    if not "%%i"=="" set "HAS_MSVC=1"
)
if "!HAS_MSVC!"=="1" goto :check_msvc_done

:check_msvc_warn
echo [AVISO] No se detecto Visual Studio Build Tools - Desarrollo para el escritorio con C++.
echo Si la compilacion falla al enlazar Rust ^(link.exe^), instalalo desde:
echo   https://visualstudio.microsoft.com/es/visual-cpp-build-tools/
echo Selecciona la carga: "Desktop development with C++" / "Desarrollo para el escritorio con C++".
echo.

:check_msvc_done
:: 5. Instalar dependencias de Node.js para Windows
echo [1/3] Instalando dependencias frontend ^(npm^)...
call npm install --no-audit --no-fund
if errorlevel 1 goto :err_npm

:: 6. Compilar frontend + Rust + instalador NSIS
echo.
echo [2/3] Compilando ZunzShot v1.0.2 ^(la primera vez puede tardar varios minutos^)...
echo       Tauri descargara NSIS si no esta en cache.
echo.
set "CFG_ARG="
if defined TAURI_SIGNING_PRIVATE_KEY goto :has_signing_key
> "%TEMP%\zunzshot-nosign.json" echo {"bundle":{"createUpdaterArtifacts":false}}
set "CFG_ARG=-c "%TEMP%\zunzshot-nosign.json""
:has_signing_key
if exist "node_modules\.bin\tauri.cmd" (
    call .\node_modules\.bin\tauri.cmd build --bundles nsis %CFG_ARG%
) else (
    call npx tauri build --bundles nsis %CFG_ARG%
)
if errorlevel 1 goto :err_build

:: 7. Copiar ejecutables finales a dist-windows\
echo.
echo [3/3] Preparando archivos finales en dist-windows\ ...
if not exist "dist-windows" mkdir "dist-windows"

if exist "src-tauri\target\release\zunzshot.exe" (
    copy /Y "src-tauri\target\release\zunzshot.exe" "dist-windows\ZunzShot.exe" >nul
)
for %%F in ("src-tauri\target\release\bundle\nsis\*.exe") do (
    copy /Y "%%F" "dist-windows\" >nul
)

echo.
echo ========================================================
echo   COMPILACION COMPLETADA EXITOSAMENTE - ZunzShot v1.0.2
echo ========================================================
echo Archivos listos en: %~dp0dist-windows
echo.
dir /b "dist-windows"
echo.
start "" explorer "%~dp0dist-windows"
pause
exit /b 0

:no_npm
echo [ERROR] No se encontro Node.js / npm en el sistema.
echo Descargalo e instalalo desde: https://nodejs.org/
echo Luego vuelve a ejecutar este archivo.
echo.
pause
exit /b 1

:no_cargo
echo [ERROR] No se encontro Rust / Cargo en el sistema.
echo Descarga rustup-init.exe desde: https://rustup.rs/
echo Usa la opcion por defecto ^(MSVC x64^), reinicia la consola y vuelve a ejecutar este .bat.
echo.
pause
exit /b 1

:err_npm
echo.
echo [ERROR] Fallo la instalacion de dependencias npm.
pause
exit /b 1

:err_build
echo.
echo [ERROR] Fallo la compilacion de Tauri para Windows.
echo Revisa el error arriba ^(si falta link.exe, instala Visual Studio Build Tools C++^).
pause
exit /b 1
