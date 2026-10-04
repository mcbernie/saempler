; Inno Setup script for the Sämpler Windows installer.
;
; Built by .github/workflows/release.yml, which passes the version and the
; folder the bundles were collected into:
;
;   ISCC.exe /DAppVersion=0.1.0 /DSourceDir=dist/saempler-windows saempler.iss
;
; Building it by hand needs the same two defines.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\target\bundled"
#endif

#define AppName "Sämpler"
#define AppPublisher "mcbernie"
#define AppURL "https://github.com/mcbernie/saempler"

[Setup]
AppId={{8F3C21D4-5B7A-4E6C-9A11-5C0E2D7B4A93}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
DefaultDirName={autopf}\{#AppPublisher}\{#AppName}
DefaultGroupName={#AppName}
; The plug-in folders are machine wide, so the installer needs to elevate.
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible
OutputDir=..\..\dist\installer
OutputBaseFilename=Saempler-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
LicenseFile=..\..\LICENSE
DisableProgramGroupPage=yes
UninstallDisplayName={#AppName} {#AppVersion}

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Types]
Name: "full"; Description: "Alles"
Name: "custom"; Description: "Benutzerdefiniert"; Flags: iscustom

[Components]
Name: "vst3"; Description: "VST3-Plug-in"; Types: full custom
Name: "clap"; Description: "CLAP-Plug-in"; Types: full custom
Name: "standalone"; Description: "Eigenständige Anwendung"; Types: full

[Files]
; The bundles are folders, not single files, so each goes in recursively.
Source: "{#SourceDir}\Sämpler.vst3\*"; DestDir: "{commoncf64}\VST3\Sämpler.vst3"; \
  Flags: ignoreversion recursesubdirs createallsubdirs; Components: vst3
Source: "{#SourceDir}\Sämpler.clap\*"; DestDir: "{commoncf64}\CLAP\Sämpler.clap"; \
  Flags: ignoreversion recursesubdirs createallsubdirs; Components: clap
Source: "{#SourceDir}\saempler.exe"; DestDir: "{app}"; \
  Flags: ignoreversion; Components: standalone
Source: "{#SourceDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\LICENSING.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\saempler.exe"; Components: standalone
Name: "{group}\{#AppName} im Netz"; Filename: "{#AppURL}"

[Run]
Filename: "{app}\saempler.exe"; Description: "{#AppName} jetzt starten"; \
  Flags: nowait postinstall skipifsilent; Components: standalone

[UninstallDelete]
; The bundle folders are created by the installer and may hold files the host
; wrote next to them, which have to go with it.
Type: filesandordirs; Name: "{commoncf64}\VST3\Sämpler.vst3"
Type: filesandordirs; Name: "{commoncf64}\CLAP\Sämpler.clap"
