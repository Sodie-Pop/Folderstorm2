; Folderstorm2's Windows installer (Inno Setup 6): per user, no admin.
;
; Plan: folderstorm/plans/releases-and-installer.md, sections 3 and 8. It
; lays down the release package that folderstorm/scripts/package-release.ps1
; makes (the same files as the zip) in %LOCALAPPDATA%\Programs\<product>, so
; the viewer's own updater (crates/aurora-updater/src/install.rs) keeps
; updating that folder from the zip: it swaps the package's entries inside
; the folder, which never moves, and writes its work folder and lock beside
; it, in %LOCALAPPDATA%\Programs, which this user can write.
;
; Built by folderstorm/scripts/build-installer.ps1, which reads the names
; from crates/folderstorm-brand/src/lib.rs and passes them as defines:
;   ISCC.exe /DAppVersion=0.1.0 /DSourceDir=<package folder> ... folderstorm2.iss
; Nothing here names the product: every name comes from a define.
;
; This file is UTF-8 with a byte order mark (the French and Japanese texts).

; Every define below is required:
; - the release: AppVersion (0.1.0, 0.2.0-beta.1), AppVersionNumeric
;   (0.1.0.0, for the file's version block), SourceDir (the package folder),
;   OutputDir;
; - from folderstorm-brand: AppName (product!()), AppExe (EXE), McpExe
;   (MCP_EXE), ReleasePrefix (RELEASE_PREFIX), Publisher (SITE_NAME),
;   WebsiteUrl (WEBSITE), HelpUrl (HELP), BugsUrl (BUG_REPORTS), PrivacyUrl
;   (PRIVACY), ReleasesUrl (RELEASES_PAGE), AumId (WINDOWS_AUMID),
;   DirsOrganization and DirsApplication (DIRS_ORGANIZATION,
;   DIRS_APPLICATION).
#if !Defined(AppVersion) || !Defined(AppVersionNumeric) || !Defined(SourceDir) || !Defined(OutputDir) \
  || !Defined(AppName) || !Defined(AppExe) || !Defined(McpExe) || !Defined(ReleasePrefix) || !Defined(Publisher) \
  || !Defined(WebsiteUrl) || !Defined(HelpUrl) || !Defined(BugsUrl) || !Defined(PrivacyUrl) || !Defined(ReleasesUrl) \
  || !Defined(AumId) || !Defined(DirsOrganization) || !Defined(DirsApplication)
  #error A define is missing: build with folderstorm/scripts/build-installer.ps1
#endif

; The package must be complete: the updater only treats the folder as a
; package (and replaces the MCP helper and assets with the viewer) when the
; manifest, the assets and the licence are there (install.rs, Layout).
#if !FileExists(SourceDir + "\" + AppExe)
  #error The package folder has no viewer executable
#endif
#if !FileExists(SourceDir + "\" + McpExe)
  #error The package folder has no MCP helper
#endif
#if !FileExists(SourceDir + "\aurora-install.json")
  #error The package folder has no aurora-install.json
#endif
#if !FileExists(SourceDir + "\LICENSE")
  #error The package folder has no LICENSE
#endif

[Setup]
; Never change this id: Windows finds an existing installation by it.
AppId={{1BB45093-F6F3-43CD-BAF0-813BCB80E9E6}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#Publisher}
AppPublisherURL={#WebsiteUrl}
AppSupportURL={#HelpUrl}
AppUpdatesURL={#ReleasesUrl}
AppContact={#BugsUrl}
AppComments=Bug reports: {#BugsUrl} - Privacy: {#PrivacyUrl}
VersionInfoVersion={#AppVersionNumeric}
VersionInfoProductTextVersion={#AppVersion}
VersionInfoDescription={#AppName} Setup
; Per user only (decision 3): the updater writes beside the installation
; folder, which C:\Program Files would refuse without admin. The folder
; %LOCALAPPDATA%\Programs is also where {userpf} points for a per-user
; install.
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename={#ReleasePrefix}-v{#AppVersion}-windows-x64-setup
SetupIconFile=..\branding\folderstorm2.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; A running viewer (or MCP helper) holds its files: Setup asks to close it
; through the Restart Manager, and does not restart it afterwards (a session
; host's characters would come back without their host). The uninstaller
; checks by itself (InitializeUninstall below).
CloseApplications=yes
CloseApplicationsFilter=*.exe
RestartApplications=no
; the secondlife:// task writes URL classes: Explorer re-reads them
ChangesAssociations=yes
SetupLogging=yes
; Code signing comes later (plan PR 6, SignPath): it signs the two exes in
; the release workflow before packaging, then this setup and its
; uninstaller. Once a sign tool named "signpath" is set up for ISCC
; (/Ssignpath=...), uncomment:
;SignTool=signpath
;SignedUninstaller=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"

[CustomMessages]
en.HostShortcut={#AppName} Session Host
en.HostComment=Start up to five characters, each in its own viewer
en.DesktopGroup=Desktop shortcuts:
en.DesktopViewer=Create a desktop shortcut for {#AppName}
en.DesktopHost=Create a desktop shortcut for {#AppName} Session Host
en.LinksGroup=Links:
en.LinksTask=Open secondlife:// links with {#AppName} (you can change this later in Preferences)
en.DriveRoot=Choose a folder inside another folder, not directly at the root of a drive: {#AppName} updates itself by writing next to its folder.
en.Newer=A newer version of {#AppName} is installed in this folder (%1). This setup would replace it with the older version %2.%n%nContinue anyway?
en.Running=Close {#AppName} first (the session host and its characters too, and any assistant using its MCP helper), then click Retry.
en.RemoveData=Also remove your {#AppName} settings, saved passwords, logs and cache from this computer?%n%nChoose No to keep them for a later installation (recommended).%n%nFolders you chose yourself in Preferences (another cache location, conversation history) and your snapshots are never removed.

fr.HostShortcut={#AppName} Hôte de session
fr.HostComment=Démarrer jusqu'à cinq personnages, chacun dans son propre viewer
fr.DesktopGroup=Raccourcis sur le bureau :
fr.DesktopViewer=Créer un raccourci sur le bureau pour {#AppName}
fr.DesktopHost=Créer un raccourci sur le bureau pour {#AppName} Hôte de session
fr.LinksGroup=Liens :
fr.LinksTask=Ouvrir les liens secondlife:// avec {#AppName} (modifiable plus tard dans les Préférences)
fr.DriveRoot=Choisissez un dossier placé dans un autre dossier, pas directement à la racine d'un lecteur : {#AppName} se met à jour en écrivant à côté de son dossier.
fr.Newer=Une version plus récente de {#AppName} est installée dans ce dossier (%1). Ce programme d'installation la remplacerait par la version %2, plus ancienne.%n%nContinuer quand même ?
fr.Running=Fermez d'abord {#AppName} (ainsi que l'hôte de session, ses personnages et tout assistant qui utilise son aide MCP), puis cliquez sur Réessayer.
fr.RemoveData=Supprimer aussi de cet ordinateur vos réglages, mots de passe enregistrés, journaux et cache de {#AppName} ?%n%nChoisissez Non pour les garder pour une installation future (recommandé).%n%nLes dossiers que vous avez choisis vous-même dans les Préférences (autre emplacement du cache, historique des conversations) et vos photos ne sont jamais supprimés.

ja.HostShortcut={#AppName} セッションホスト
ja.HostComment=最大 5 人のキャラクターを、それぞれ専用のビューアで起動します
ja.DesktopGroup=デスクトップのショートカット:
ja.DesktopViewer={#AppName} のショートカットをデスクトップに作成する
ja.DesktopHost={#AppName} セッションホストのショートカットをデスクトップに作成する
ja.LinksGroup=リンク:
ja.LinksTask=secondlife:// リンクを {#AppName} で開く（あとで初期設定から変更できます）
ja.DriveRoot=ドライブのルート直下ではなく、別のフォルダの中にあるフォルダを選んでください。{#AppName} は自分のフォルダの隣に書き込んで更新します。
ja.Newer=このフォルダには新しいバージョンの {#AppName} (%1) がインストールされています。このセットアップは、古いバージョン %2 に置き換えます。%n%n続行しますか？
ja.Running=先に {#AppName} を閉じてください（セッションホストとそのキャラクター、MCP ヘルパーを使っているアシスタントも）。その後「再試行」をクリックしてください。
ja.RemoveData=このコンピュータから {#AppName} の設定、保存したパスワード、ログ、キャッシュも削除しますか？%n%n今後のインストールのために残すには「いいえ」を選んでください（推奨）。%n%n初期設定で自分で選んだフォルダ（別のキャッシュの場所、会話履歴）とスナップショットは削除されません。

[Tasks]
; both unticked (plan section 3)
Name: "desktopicon"; Description: "{cm:DesktopViewer}"; GroupDescription: "{cm:DesktopGroup}"; Flags: unchecked
Name: "desktophosticon"; Description: "{cm:DesktopHost}"; GroupDescription: "{cm:DesktopGroup}"; Flags: unchecked
; unticked (decision 4): it would take the links away from another viewer
Name: "links"; Description: "{cm:LinksTask}"; GroupDescription: "{cm:LinksGroup}"; Flags: unchecked

[InstallDelete]
; Over an installation an update has changed, start from the package alone,
; as an update does: it replaces the whole assets folder.
Type: filesandordirs; Name: "{app}\assets"
Type: files; Name: "{app}\version.txt"

[Files]
; the package folder exactly: Folderstorm2.exe, folderstorm-mcp.exe,
; assets\, LICENSE, NOTICE.md, README.md, aurora-install.json
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
; No AppUserModelID on the shortcuts: the viewer sets none on its window, so
; a pinned shortcut would show as a second taskbar button (plan section 3).
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"
; --session-host: crates/aurora-viewer/src/cli.rs. An update restarts the
; viewer with the same arguments (install.rs), so the host stays a host.
Name: "{group}\{cm:HostShortcut}"; Filename: "{app}\{#AppExe}"; Parameters: "--session-host"; WorkingDir: "{app}"; Comment: "{cm:HostComment}"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Tasks: desktopicon
Name: "{autodesktop}\{cm:HostShortcut}"; Filename: "{app}\{#AppExe}"; Parameters: "--session-host"; WorkingDir: "{app}"; Comment: "{cm:HostComment}"; Tasks: desktophosticon

[Registry]
; The secondlife:// handler, exactly as the viewer writes it
; (crates/aurora-viewer/src/url_scheme.rs, win::register_scheme): same
; classes under HKCU, same values, same command. The viewer then sees it as
; its own (Preferences › Interface can turn it off), and its own removal
; (--uninstall) deletes it. No uninsdeletekey flag: the uninstaller deletes
; a class only while it still opens this installation (RemoveOurLinks).
Root: HKCU; Subkey: "Software\Classes\secondlife"; ValueType: string; ValueName: ""; ValueData: "URL:Virtual World"; Tasks: links
Root: HKCU; Subkey: "Software\Classes\secondlife"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKCU; Subkey: "Software\Classes\secondlife\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"",0"; Tasks: links
Root: HKCU; Subkey: "Software\Classes\secondlife\shell\open"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Tasks: links
Root: HKCU; Subkey: "Software\Classes\secondlife\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" -url ""%1"""; Tasks: links
Root: HKCU; Subkey: "Software\Classes\hop"; ValueType: string; ValueName: ""; ValueData: "URL:Virtual World"; Tasks: links
Root: HKCU; Subkey: "Software\Classes\hop"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKCU; Subkey: "Software\Classes\hop\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"",0"; Tasks: links
Root: HKCU; Subkey: "Software\Classes\hop\shell\open"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Tasks: links
Root: HKCU; Subkey: "Software\Classes\hop\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" -url ""%1"""; Tasks: links
Root: HKCU; Subkey: "Software\Classes\x-grid-location-info"; ValueType: string; ValueName: ""; ValueData: "URL:Virtual World"; Tasks: links
Root: HKCU; Subkey: "Software\Classes\x-grid-location-info"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: links
Root: HKCU; Subkey: "Software\Classes\x-grid-location-info\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"",0"; Tasks: links
Root: HKCU; Subkey: "Software\Classes\x-grid-location-info\shell\open"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Tasks: links
Root: HKCU; Subkey: "Software\Classes\x-grid-location-info\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" -url ""%1"""; Tasks: links

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; what an update may have added that Setup did not install (it replaces
; the whole assets folder), then the folder itself when nothing else is left
Type: filesandordirs; Name: "{app}\assets"
Type: files; Name: "{app}\version.txt"
Type: dirifempty; Name: "{app}"

[Code]
const
  ReparsePoint = $400;

var
  RemoveData: Boolean;

{ --- Versions ---------------------------------------------------------- }

function Pad(N: Integer): String;
begin
  Result := IntToStr(N);
  while Length(Result) < 6 do
    Result := '0' + Result;
end;

{ The next dot-separated number of S, removed from it. }
function TakeField(var S: String): Integer;
var
  P: Integer;
begin
  P := Pos('.', S);
  if P = 0 then begin
    Result := StrToIntDef(S, 0);
    S := '';
  end else begin
    Result := StrToIntDef(Copy(S, 1, P - 1), 0);
    S := Copy(S, P + 1, Length(S));
  end;
end;

{ A key that sorts as the versions do: a.b.c, then a stable version above
  its betas (-beta.N), as the updater orders them (SemVer). }
function VersionKey(V: String): String;
var
  Core, Pre: String;
  P, Major, Minor, Patch, Beta: Integer;
begin
  P := Pos('-', V);
  if P > 0 then begin
    Core := Copy(V, 1, P - 1);
    Pre := Copy(V, P + 1, Length(V));
  end else begin
    Core := V;
    Pre := '';
  end;
  Major := TakeField(Core);
  Minor := TakeField(Core);
  Patch := TakeField(Core);
  if Pre = '' then
    Beta := 999999
  else if Pos('beta.', Pre) = 1 then
    Beta := StrToIntDef(Copy(Pre, 6, Length(Pre)), 0)
  else
    Beta := 0;
  Result := Pad(Major) + Pad(Minor) + Pad(Patch) + Pad(Beta);
end;

{ The "version" of a package's aurora-install.json (written by
  package-release.ps1, replaced by every update), or ''. }
function InstalledVersion(const Dir: String): String;
var
  Text: AnsiString;
  S: String;
  P: Integer;
begin
  Result := '';
  if not LoadStringFromFile(AddBackslash(Dir) + 'aurora-install.json', Text) then
    exit;
  S := String(Text);
  P := Pos('"version"', S);
  if P = 0 then
    exit;
  S := Copy(S, P + Length('"version"'), Length(S));
  P := Pos('"', S);
  if P = 0 then
    exit;
  S := Copy(S, P + 1, Length(S));
  P := Pos('"', S);
  if P = 0 then
    exit;
  Result := Copy(S, 1, P - 1);
end;

{ --- Setup ------------------------------------------------------------- }

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Dir, Installed: String;
begin
  Result := True;
  if CurPageID = wpSelectDir then begin
    { The updater refuses a folder directly at a drive's root: it writes its
      work folder and lock in the folder above (install.rs). }
    Dir := RemoveBackslashUnlessRoot(WizardDirValue);
    if CompareText(AddBackslash(ExtractFileDrive(Dir)), AddBackslash(ExtractFileDir(Dir))) = 0 then begin
      SuppressibleMsgBox(CustomMessage('DriveRoot'), mbError, MB_OK, IDOK);
      Result := False;
    end;
  end else if CurPageID = wpReady then begin
    { The viewer may have updated itself past this setup's version. }
    Installed := InstalledVersion(WizardDirValue);
    if (Installed <> '') and (CompareStr(VersionKey(Installed), VersionKey('{#AppVersion}')) > 0) then
      Result := SuppressibleMsgBox(FmtMessage(CustomMessage('Newer'), [Installed, '{#AppVersion}']),
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES;
  end;
end;

{ --- Uninstall --------------------------------------------------------- }

function AppExePath: String;
begin
  Result := ExpandConstant('{app}\{#AppExe}');
end;

{ How many processes run this executable (WMI; 0 when WMI cannot tell). }
function RunningCount(Path: String): Integer;
var
  Locator, Service, Found: Variant;
begin
  Result := 0;
  StringChangeEx(Path, '\', '\\', True);
  StringChangeEx(Path, '''', '\''', True);
  try
    Locator := CreateOleObject('WbemScripting.SWbemLocator');
    Service := Locator.ConnectServer('.', 'root\CIMV2');
    Found := Service.ExecQuery('SELECT ProcessId FROM Win32_Process WHERE ExecutablePath = ''' + Path + '''');
    Result := Found.Count;
  except
    Log('Cannot list the running processes: ' + GetExceptionMessage);
  end;
end;

function ViewerRunning: Boolean;
begin
  Result := (RunningCount(AppExePath) > 0) or (RunningCount(ExpandConstant('{app}\{#McpExe}')) > 0);
end;

function InitializeUninstall: Boolean;
begin
  Result := True;
  while ViewerRunning do begin
    if UninstallSilent then begin
      Log('The viewer or its MCP helper is running: uninstall stopped');
      Result := False;
      exit;
    end;
    if MsgBox(CustomMessage('Running'), mbError, MB_RETRYCANCEL) <> IDRETRY then begin
      Result := False;
      exit;
    end;
  end;
end;

{ The program of a `"<exe>" -url "%1"` command, as url_scheme.rs reads it
  (command_exe). }
function CommandExe(Command: String): String;
var
  P: Integer;
begin
  Command := Trim(Command);
  if (Length(Command) > 0) and (Command[1] = '"') then begin
    Command := Copy(Command, 2, Length(Command));
    P := Pos('"', Command);
    if P > 0 then
      Command := Copy(Command, 1, P - 1);
  end else begin
    P := Pos(' ', Command);
    if P > 0 then
      Command := Copy(Command, 1, P - 1);
  end;
  Result := Command;
end;

{ The link classes that still open this installation's viewer, whether this
  setup or the viewer's Preferences wrote them; another viewer's stay, as
  the viewer's own removal leaves them (url_scheme.rs, ours). }
procedure RemoveOurLinks;
var
  Schemes: TArrayOfString;
  I: Integer;
  Key, Command: String;
begin
  SetArrayLength(Schemes, 3);
  Schemes[0] := 'secondlife';
  Schemes[1] := 'hop';
  Schemes[2] := 'x-grid-location-info';
  for I := 0 to GetArrayLength(Schemes) - 1 do begin
    Key := 'Software\Classes\' + Schemes[I];
    if RegQueryStringValue(HKCU, Key + '\shell\open\command', '', Command)
      and (CompareText(CommandExe(Command), AppExePath) = 0) then begin
      if RegDeleteKeyIncludingSubkeys(HKCU, Key) then
        Log('Removed the ' + Schemes[I] + ' link class')
      else
        Log('Cannot remove the ' + Schemes[I] + ' link class');
    end;
  end;
end;

{ What the updater leaves beside the installation (install.rs): its lock
  ".<folder>.update.lock", and a ".aurora-update-<uuid>" folder per update
  it could not tidy (one still holding "backup" is the previous version
  after a failed restore). Only this installation's: the folder's plan.json
  names it ("install": "...\\<folder>", then "token"); another program's,
  in the same Programs folder, stays. }
procedure RemoveUpdateLeftovers;
var
  App, Parent, Needle, Work: String;
  Plan: AnsiString;
  Found: TFindRec;
begin
  App := RemoveBackslashUnlessRoot(ExpandConstant('{app}'));
  Parent := ExtractFileDir(App);
  Needle := Lowercase('\\' + ExtractFileName(App) + '","token"');
  if FindFirst(AddBackslash(Parent) + '.aurora-update-*', Found) then begin
    try
      repeat
        Work := AddBackslash(Parent) + Found.Name;
        if ((Found.Attributes and FILE_ATTRIBUTE_DIRECTORY) <> 0)
          and ((Found.Attributes and ReparsePoint) = 0)
          and LoadStringFromFile(Work + '\plan.json', Plan)
          and (Pos(Needle, Lowercase(String(Plan))) > 0) then begin
          if DelTree(Work, True, True, True) then
            Log('Removed the update folder ' + Work)
          else
            Log('Cannot remove the update folder ' + Work);
        end;
      until not FindNext(Found);
    finally
      FindClose(Found);
    end;
  end;
  DeleteFile(AddBackslash(Parent) + '.' + ExtractFileName(App) + '.update.lock');
end;

{ The default folders of folderstorm_brand::project_dirs() on Windows:
  settings in %APPDATA%\<org>\<app>, cache, logs and media plugins in
  %LOCALAPPDATA%\<org>\<app> (named profiles and their caches inside). The
  <org> folder goes only when empty: another program of the same name may
  use it. Folders chosen in Preferences are never deleted. }
procedure RemoveUserData;
var
  Roots: TArrayOfString;
  I: Integer;
begin
  SetArrayLength(Roots, 2);
  Roots[0] := ExpandConstant('{userappdata}\{#DirsOrganization}');
  Roots[1] := ExpandConstant('{localappdata}\{#DirsOrganization}');
  for I := 0 to GetArrayLength(Roots) - 1 do begin
    if DirExists(Roots[I] + '\{#DirsApplication}') then begin
      if DelTree(Roots[I] + '\{#DirsApplication}', True, True, True) then
        Log('Removed ' + Roots[I] + '\{#DirsApplication}')
      else
        Log('Cannot remove all of ' + Roots[I] + '\{#DirsApplication}');
    end;
    RemoveDir(Roots[I]);
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Code: Integer;
begin
  if CurUninstallStep = usUninstall then begin
    { Default No (decision 5); a silent uninstall keeps everything. }
    RemoveData := (not UninstallSilent)
      and (MsgBox(CustomMessage('RemoveData'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES);
    if RemoveData and FileExists(AppExePath) then begin
      { The viewer's own removal (crates/aurora-viewer/src/uninstall.rs),
        while its files are still here: link classes that open it, the
        notification identity, saved passwords, second-factor tokens, the
        cache key, the crash reports' and proxy's credentials, the Blender
        add-on. It deletes no folder: RemoveUserData does, after. }
      if Exec(AppExePath, '--uninstall', ExpandConstant('{app}'), SW_HIDE, ewWaitUntilTerminated, Code) then
        Log('--uninstall exit code: ' + IntToStr(Code))
      else
        Log('--uninstall did not start: ' + SysErrorMessage(Code));
    end;
    { Whatever the answer: nothing may keep pointing at the deleted viewer.
      The notification identity is written again by any copy that starts. }
    RemoveOurLinks;
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Classes\AppUserModelId\{#AumId}');
  end else if CurUninstallStep = usPostUninstall then begin
    RemoveUpdateLeftovers;
    if RemoveData then
      RemoveUserData;
  end;
end;
