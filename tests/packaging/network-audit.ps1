<#
Metadata-only, best-effort process-tree sampler. No packets, new DNS lookups,
command lines, executable paths, Maxed settings, history or recordings are read.
Pass means no disallowed endpoints were OBSERVED in a complete sample window;
it is never proof that connections between samples did not occur.
#>
[CmdletBinding(DefaultParameterSetName = 'Audit')]
param(
    [Parameter(Mandatory, ParameterSetName = 'SelfTest')][switch]$SelfTest,
    [Parameter(Mandatory, ParameterSetName = 'Audit')][ValidateRange(1, 2147483647)][int]$RootProcessId,
    [Parameter(Mandatory, ParameterSetName = 'Audit')][string]$OutputDir,
    [Parameter(ParameterSetName = 'Audit')][ValidateRange(1, 86400)][int]$Seconds = 60,
    [Parameter(ParameterSetName = 'Audit')][ValidatePattern('^[a-zA-Z0-9_-]{1,64}$')][string]$Phase = 'unspecified',
    [Parameter(ParameterSetName = 'Audit')][ValidateSet('app', 'update', 'installer')][string]$Policy = 'app'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$script:intervalMs = 250
# Cmdlets achieve about one sample per second here; 3000 ms leaves headroom
# without accepting longer multi-second blind windows.
Set-Variable -Name CoverageGapLimitMs -Scope Script -Option Constant -Value 3000
# Owner decision 2026-10-04: disclose and allow these WebView2 runtime hosts;
# see docs/release/network-audit.md. Exact names only, never suffixes/wildcards.
Set-Variable -Name WebView2RuntimeHosts -Scope Script -Option Constant -Value @(
    'substrate.office.com', 'config.edge.skype.com', 'edge.microsoft.com'
)
# Component payload downloads are part of the same owner decision (2026-10-04).
# Exact b/f delivery hosts: https://learn.microsoft.com/en-us/deployedge/microsoft-edge-security-endpoints#other-browser-support-services
# Port 80 is accepted ONLY here because component payloads are signed: the CRX3
# verifier checks required keys and signatures before the updater unpacks them.
# https://github.com/chromium/chromium/blob/main/components/update_client/unpacker.cc
# https://github.com/chromium/chromium/blob/main/components/crx_file/crx_verifier.cc
# This protects payload authenticity/integrity, not HTTP confidentiality.
Set-Variable -Name WebView2ComponentDownloadHosts -Scope Script -Option Constant -Value @(
    'msedge.b.tlu.dl.delivery.mp.microsoft.com', 'msedge.f.tlu.dl.delivery.mp.microsoft.com',
    'msedge.b.dl.delivery.mp.microsoft.com', 'msedge.f.dl.delivery.mp.microsoft.com'
)

function Get-RuntimeHostsForPort([int]$Port) {
    if ($Port -eq 443) { return @($script:WebView2RuntimeHosts) + @($script:WebView2ComponentDownloadHosts) }
    if ($Port -eq 80) { return $script:WebView2ComponentDownloadHosts }
    return @()
}

function Add-Reason($Report, [string]$Code) {
    if ($Code -cnotin $Report.reasons) { $Report.reasons += $Code }
}

function New-NetworkReport([string]$SelectedPolicy, [string]$Label) {
    [pscustomobject][ordered]@{
        schema_version = 2; result = 'not_measured'; policy = $SelectedPolicy; phase = $Label
        started_utc = [DateTime]::UtcNow.ToString('o'); ended_utc = $null
        method = 'Get-NetTCPConnection + Get-NetUDPEndpoint; Win32_Process inventory and Get-Process StartTime before/after; local DNS metadata only'
        tools = [ordered]@{ powershell = $PSVersionTable.PSVersion.ToString(); windows = [Environment]::OSVersion.Version.ToString(); NetTCPIP = $null; DnsClient = $null; CimCmdlets = $null }
        sample_interval_ms = $script:intervalMs; samples = 0; requested_seconds = 0; elapsed_seconds = 0.0
        max_sample_duration_ms = 0.0; max_sample_start_gap_ms = 0.0
        coverage_gap_limit_ms = $script:CoverageGapLimitMs
        leading_coverage_gap_ms = 0.0; trailing_coverage_gap_ms = 0.0; max_coverage_gap_ms = 0.0
        dns = [ordered]@{ operational_log = 'not_probed'; cache = 'not_probed'; correlation = 'system-wide IP-to-name candidates, not process-specific or TLS proof' }
        proxy = [ordered]@{ configured = 'not_probed'; auto_detect = $null; method = 'WinHTTP default + current-user WinINet configuration APIs; WinHTTP advanced user/machine settings (netsh)'; checks = 0 }
        reasons = @(); processes = @(); endpoints = @(); udp_bindings = @()
        runtime_allowances = @((Get-RuntimeHostsForPort 443) | ForEach-Object { [pscustomobject]@{ host = $_; observations = 0 } })
        runtime_unresolved_observations = 0
        runtime_ambiguous_observations = 0
        blind_spots = @(
            'Polling misses short-lived connections and descendants between samples, including traffic before attachment and after collection.',
            'Coverage includes timer start to first sample start, consecutive sample starts, and last sample start to collection end. A gap over 3000 ms or fewer than ceil(requested_seconds * 1000 / 3000) complete samples makes coverage insufficient; smaller gaps still miss short-lived traffic.',
            'TCP tables contain sockets, not requests, bytes, direction or TLS SNI; inbound accepted sockets cannot be distinguished from outbound connects.',
            'UDP endpoint tables expose only local bindings: no remote peer, datagram count or QUIC destination; non-loopback owned bindings invalidate completeness.',
            'Local DNS cache/log correlation is system-wide, may be stale, ambiguous, incomplete or absent (DoH, literal IP, proxy); no reverse lookup is issued.',
            'WebView2 runtime allowance requires every correlated name to be approved for the port: service hosts on 443, signed component download hosts on 80/443. Mixed approved/unapproved names are inconclusive; endpoints with no approved names violate policy. All names are retained, but requested host and payload verification cannot be proved by this sampler. Per-host counts count socket observations and can overlap, not requests.',
            'Proxies/PAC/WPAD, VPNs and local forwarding agents can mask external destinations. Manual proxy/PAC configuration or loopback peers outside the owned tree invalidate completeness; transparent redirection and changes between configuration checks remain blind spots.',
            'Auto-detect alone is recorded without blocking pass. No WPAD discovery or PAC evaluation is performed. Discovered proxy endpoints still face the destination and loopback-peer rules: LAN/unapproved public proxies violate even update policy; foreign loopback peers are not_measured, never approved update delivery.',
            'HTTPS, exact repository/release paths, redirect paths/count and conditional bootstrap purpose cannot be proven by these counters. Approved-host candidates require separate transport evidence.',
            'Preexisting shared Microsoft updater/services and elevated/reparented children never observed under the root are outside process-tree attribution.',
            'A root exit during collection is not_measured, including normal installer exit/shutdown; collect those windows with an additional metadata trace.'
        )
    }
}

function Assert-NetworkOutput([string]$Directory) {
    $full = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Directory)
    if (-not (Test-Path -LiteralPath $full -PathType Container)) { throw 'output_missing' }
    $item = Get-Item -LiteralPath $full -Force
    $cursor = $item
    while ($null -ne $cursor) {
        if ($cursor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'output_reparse' }
        $cursor = $cursor.Parent
    }
    if (@(Get-ChildItem -LiteralPath $full -Force).Count -ne 0) { throw 'output_not_empty' }
    return $full
}

function Get-SafeHost([string]$Value) {
    # Accept a DNS name only, never a URL/path/query or arbitrary diagnostic.
    $name = $Value.TrimEnd('.').ToLowerInvariant()
    if ($name.Length -gt 253 -or $name -notmatch '^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$') { return $null }
    foreach ($label in $name.Split('.')) {
        if ($label.Length -lt 1 -or $label.Length -gt 63 -or $label -notmatch '^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$') { return $null }
    }
    return $name
}

function Get-IP([string]$Value) {
    $address = $null
    if ([Net.IPAddress]::TryParse($Value, [ref]$address)) {
        if ($address.IsIPv4MappedToIPv6) { $address = $address.MapToIPv4() }
        return $address
    }
    return $null
}

function Test-LocalNetworkIP($Address) {
    $bytes = $Address.GetAddressBytes()
    if ($bytes.Length -eq 4) {
        return ($bytes[0] -eq 10 -or ($bytes[0] -eq 172 -and $bytes[1] -ge 16 -and $bytes[1] -le 31) -or
            ($bytes[0] -eq 192 -and $bytes[1] -eq 168) -or ($bytes[0] -eq 169 -and $bytes[1] -eq 254))
    }
    return ($Address.IsIPv6LinkLocal -or (($bytes[0] -band 254) -eq 252))
}

function Get-EndpointClass([string]$Address, [int]$Port, [string[]]$Hosts, [string]$SelectedPolicy, [string]$Role = 'unknown') {
    $ip = Get-IP $Address
    if ($null -eq $ip) { throw 'endpoint_invalid' }
    if ([Net.IPAddress]::IsLoopback($ip)) { return 'loopback_local_allowed' }
    if (Test-LocalNetworkIP $ip) { return 'local_network_violation' }
    if ($Role -ceq 'webview2') {
        if ($Port -notin @(80, 443)) { return 'non_https_port_violation' }
        if ($Hosts.Count -eq 0) { return 'runtime_host_unresolved' }
        $runtimeHosts = @(Get-RuntimeHostsForPort $Port)
        $approved = $false; $unapproved = $false
        foreach ($name in $Hosts) {
            if ((Get-SafeHost $name) -cin $runtimeHosts) { $approved = $true }
            else { $unapproved = $true }
        }
        # Shared/CDN IPs cannot authorize a co-tenant or a service on the wrong
        # port. Preserve ambiguous evidence instead of guessing the TLS/HTTP host.
        if ($approved -and $unapproved) { return 'runtime_host_ambiguous' }
        if ($approved) { return 'webview2_runtime_allowed' }
        return 'unapproved_host_violation'
    }
    if ($Role -cnotin @('maxed-root', 'installer-root', 'microsoft-bootstrap-runtime')) { return 'unknown_role_violation' }
    if ($SelectedPolicy -eq 'update' -and $Role -cne 'maxed-root') { return 'unapproved_role_violation' }
    if ($Hosts.Count -eq 0) { return 'ip_only_violation' }
    $updateHosts = @('github.com', 'release-assets.githubusercontent.com')
    # Exact documented delivery names only. No *.microsoft.com wildcard,
    # telemetry exemption, CNAME alias authorization or ordinary Edge browsing.
    $installerHosts = @('go.microsoft.com', 'msedge.api.cdp.microsoft.com',
        'msedge.sf.dl.delivery.mp.microsoft.com', 'msedge.sf.tlu.dl.delivery.mp.microsoft.com',
        'msedge.sb.dl.delivery.mp.microsoft.com', 'msedge.sb.tlu.dl.delivery.mp.microsoft.com')
    $installerHttpHosts = @('msedge.f.dl.delivery.mp.microsoft.com', 'msedge.f.tlu.dl.delivery.mp.microsoft.com',
        'msedge.b.dl.delivery.mp.microsoft.com', 'msedge.b.tlu.dl.delivery.mp.microsoft.com')
    if ($SelectedPolicy -eq 'installer' -and $Port -eq 80) { $installerHosts = $installerHttpHosts }
    elseif ($Port -ne 443) { return 'non_https_port_violation' }
    $allowed = switch ($SelectedPolicy) { 'update' { $updateHosts }; 'installer' { $installerHosts }; default { @() } }
    foreach ($hostName in $Hosts) {
        if ((Get-SafeHost $hostName) -cnotin $allowed) { return 'unapproved_host_violation' }
    }
    if ($SelectedPolicy -eq 'update') { return 'approved_update_origin_candidate' }
    if ($SelectedPolicy -eq 'installer') { return 'microsoft_distribution_candidate' }
    return 'unapproved_host_violation'
}

function Get-ProcessRole([string]$Name, [bool]$IsRoot, [string]$SelectedPolicy) {
    if ($IsRoot) { if ($SelectedPolicy -eq 'installer') { return 'installer-root' }; return 'maxed-root' }
    switch ($Name.ToLowerInvariant()) {
        'msedgewebview2.exe' { return 'webview2' }
        'microsoftedgeupdate.exe' { return 'microsoft-bootstrap-runtime' }
        'microsoftedgewebview2setup.exe' { return 'microsoft-bootstrap-runtime' }
        default { return 'other-descendant' }
    }
}

function Test-CimBirth($InventoryBirth, [DateTime]$ProcessBirth) {
    if ($null -eq $InventoryBirth) { return $false }
    # CIM_DATETIME has six fractional digits; Get-Process uses 100 ns ticks.
    # Compare only their shared microsecond precision. Keep the full StartTime
    # in Known and compare it exactly before/after sampling to reject PID reuse.
    $utc = $ProcessBirth.ToUniversalTime()
    return ($InventoryBirth.ToUniversalTime().Ticks -eq ($utc.Ticks - ($utc.Ticks % 10)))
}

function Get-ProcessKey([int]$ProcessId, [DateTime]$Born) {
    return '{0}|{1}' -f $ProcessId, $Born.ToUniversalTime().Ticks
}

function Read-InventoryIdentity($Entry, [int]$RootId, [scriptblock]$BirthReader, [string]$SelectedPolicy) {
    $id = [int]$Entry.ProcessId
    if ($null -eq $Entry.CreationDate) { throw 'identity_missing' }
    try { $born = & $BirthReader $id }
    catch { if ($id -eq $RootId) { throw 'root_lost' }; throw 'identity_missing' }
    if ($null -eq $born -or -not (Test-CimBirth $Entry.CreationDate $born)) { throw 'identity_changed' }
    return [pscustomobject][ordered]@{
        pid = $id; parent_pid = [int]$Entry.ParentProcessId; born = $born
        role = Get-ProcessRole $Entry.Name ($id -eq $RootId) $SelectedPolicy
    }
}

function Read-OwnedTree($Inventory, [hashtable]$Known, [int]$RootId, [scriptblock]$BirthReader, [string]$SelectedPolicy) {
    $listed = @{}
    foreach ($entry in $Inventory) {
        $key = [int]$entry.ProcessId
        if ($listed.ContainsKey($key)) { throw 'inventory_duplicate' }
        $listed[$key] = $entry
    }
    if (-not $listed.ContainsKey($RootId)) { throw 'root_lost' }
    # Pin the original root: replacing it is a lost root, not a new audit target.
    $rows = @{}; $identities = @{}
    $root = Read-InventoryIdentity $listed[$RootId] $RootId $BirthReader $SelectedPolicy
    foreach ($previous in $Known.Values) {
        if ($previous.pid -eq $RootId -and $previous.born -ne $root.born) { throw 'root_lost' }
    }
    $rows[$RootId] = $root; $identities[$RootId] = $root
    foreach ($previous in $Known.Values) {
        $id = $previous.pid
        if ($id -eq $RootId -or -not $listed.ContainsKey($id)) { continue }
        if (-not (Test-CimBirth $listed[$id].CreationDate $previous.born)) { continue }
        if (-not $identities.ContainsKey($id)) {
            $identities[$id] = Read-InventoryIdentity $listed[$id] $RootId $BirthReader $SelectedPolicy
        }
        # Retain reparented children only when the full identity still matches.
        # A recycled PID is evaluated below using its current parentage.
        if ($identities[$id].born -eq $previous.born) { $rows[$id] = $identities[$id] }
    }
    do {
        $changed = $false
        foreach ($entry in $Inventory) {
            $key = [int]$entry.ProcessId
            $parentId = [int]$entry.ParentProcessId
            if ($rows.ContainsKey($parentId) -and -not $rows.ContainsKey($key)) {
                if ($null -eq $entry.CreationDate) { throw 'identity_missing' }
                $parentBirth = $rows[$parentId].born
                # The OS keeps the creator PID after that creator exits. An older
                # process cannot be a child of a newer process reusing that PID.
                $creation = $entry.CreationDate.ToUniversalTime().Ticks
                if ($creation -lt ($root.born.Ticks - ($root.born.Ticks % 10)) -or
                    $creation -lt ($parentBirth.Ticks - ($parentBirth.Ticks % 10))) { continue }
                if (-not $identities.ContainsKey($key)) {
                    $identities[$key] = Read-InventoryIdentity $entry $RootId $BirthReader $SelectedPolicy
                }
                $row = $identities[$key]
                if ($row.born -lt $root.born -or $row.born -lt $parentBirth) { continue }
                $rows[$key] = $row; $changed = $true
            }
        }
    } while ($changed)
    foreach ($row in $rows.Values) {
        $identityKey = Get-ProcessKey $row.pid $row.born
        if (-not $Known.ContainsKey($identityKey)) { $Known[$identityKey] = $row }
    }
    return $rows
}

function Read-Birth([int]$ProcessId) {
    $process = Get-Process -Id $ProcessId -ErrorAction Stop
    try { return $process.StartTime.ToUniversalTime() } finally { $process.Dispose() }
}

function Initialize-ProxyReader {
    if ('MaxedNetworkProxy' -as [type]) { return }
    # Configuration only: no discovery, PAC evaluation, URL resolution or requests.
    # Native pointers are inspected for presence and freed, never copied to logs.
    Add-Type -ErrorAction Stop -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class MaxedNetworkProxy {
    [StructLayout(LayoutKind.Sequential)] struct IEConfig {
        public int AutoDetect; public IntPtr AutoConfigUrl, Proxy, Bypass;
    }
    [StructLayout(LayoutKind.Sequential)] struct DefaultConfig {
        public uint AccessType; public IntPtr Proxy, Bypass;
    }
    [DllImport("winhttp.dll", SetLastError=true)]
    static extern bool WinHttpGetIEProxyConfigForCurrentUser(ref IEConfig config);
    [DllImport("winhttp.dll", SetLastError=true)]
    static extern bool WinHttpGetDefaultProxyConfiguration(ref DefaultConfig config);
    [DllImport("kernel32.dll")] static extern IntPtr GlobalFree(IntPtr value);
    static bool HasText(IntPtr p) { return p != IntPtr.Zero && Marshal.ReadInt16(p) != 0; }
    static void Free(IntPtr p) { if (p != IntPtr.Zero) GlobalFree(p); }
    public static bool[] Read() {
        var ie = new IEConfig(); var http = new DefaultConfig();
        try {
            bool ieOK = WinHttpGetIEProxyConfigForCurrentUser(ref ie);
            bool httpOK = WinHttpGetDefaultProxyConfiguration(ref http);
            return new bool[] {ieOK && httpOK,
                httpOK && (http.AccessType != 1 || HasText(http.Proxy)),
                ieOK && HasText(ie.Proxy), ieOK && ie.AutoDetect != 0,
                ieOK && HasText(ie.AutoConfigUrl)};
        } finally {
            Free(ie.Proxy); Free(ie.Bypass); Free(ie.AutoConfigUrl);
            Free(http.Proxy); Free(http.Bypass);
        }
    }
}
'@
}

function ConvertFrom-AdvancedProxy([string]$Text) {
    # The localized heading is discarded; only the JSON's flags are retained.
    # No raw netsh output, proxy address, bypass list or PAC URL leaves this function.
    $start = $Text.IndexOf('{'); $end = $Text.LastIndexOf('}')
    if ($start -lt 0 -or $end -le $start) { throw 'proxy_configuration_unavailable' }
    try { $config = $Text.Substring($start, $end - $start + 1) | ConvertFrom-Json }
    catch { throw 'proxy_configuration_unavailable' }
    if ($config.AutoDetect -isnot [bool]) { throw 'proxy_configuration_unavailable' }
    if ($null -ne $config.PSObject.Properties['ProxyIsEnabled'] -and
        $null -ne $config.PSObject.Properties['AutoConfigIsEnabled']) {
        if ($config.ProxyIsEnabled -isnot [bool] -or $config.AutoConfigIsEnabled -isnot [bool]) { throw 'proxy_configuration_unavailable' }
        return [pscustomobject]@{ manual = $config.ProxyIsEnabled; pac = $config.AutoConfigIsEnabled; auto_detect = $config.AutoDetect }
    }
    if ($null -eq $config.PSObject.Properties['Proxy'] -or $config.Proxy -isnot [string] -or
        $null -eq $config.PSObject.Properties['AutoconfigUrl'] -or $config.AutoconfigUrl -isnot [string]) { throw 'proxy_configuration_unavailable' }
    return [pscustomobject]@{ manual = -not [string]::IsNullOrWhiteSpace($config.Proxy); pac = -not [string]::IsNullOrWhiteSpace($config.AutoconfigUrl); auto_detect = $config.AutoDetect }
}

function Read-AdvancedProxy([ValidateSet('user', 'machine')][string]$Scope) {
    $process = New-Object Diagnostics.Process
    $process.StartInfo.FileName = Join-Path $env:SystemRoot 'System32/netsh.exe'
    $process.StartInfo.Arguments = 'winhttp show advproxy setting-scope=' + $Scope
    $process.StartInfo.UseShellExecute = $false; $process.StartInfo.CreateNoWindow = $true
    $process.StartInfo.RedirectStandardOutput = $true; $process.StartInfo.RedirectStandardError = $true
    try {
        $null = $process.Start()
        $stdout = $process.StandardOutput.ReadToEndAsync(); $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(5000)) { $process.Kill(); throw 'proxy_configuration_unavailable' }
        if ($process.ExitCode -ne 0) { throw 'proxy_configuration_unavailable' }
        return ConvertFrom-AdvancedProxy $stdout.GetAwaiter().GetResult()
    } finally { $process.Dispose() }
}

function Read-LocalProxy {
    $status = [pscustomobject]@{ available = $true; winhttp = $false; wininet = $false; auto_detect = $false; pac = $false }
    try {
        Initialize-ProxyReader
        $native = [MaxedNetworkProxy]::Read()
        $status.available = $native[0]; $status.winhttp = $native[1]; $status.wininet = $native[2]
        $status.auto_detect = $native[3]; $status.pac = $native[4]
    } catch { $status.available = $false }
    foreach ($scope in @('user', 'machine')) {
        try {
            $advanced = Read-AdvancedProxy $scope
            $status.winhttp = $status.winhttp -or $advanced.manual
            $status.auto_detect = $status.auto_detect -or $advanced.auto_detect
            $status.pac = $status.pac -or $advanced.pac
        } catch { $status.available = $false }
    }
    return $status
}

function Add-ProxyStatus($Report, $Status) {
    $Report.proxy.checks++
    foreach ($field in @('available', 'winhttp', 'wininet', 'auto_detect', 'pac')) {
        if ($null -eq $Status -or $null -eq $Status.PSObject.Properties[$field] -or $Status.$field -isnot [bool]) {
            Add-Reason $Report 'proxy_configuration_unavailable'
            if ($Report.proxy.configured -ne 'yes') { $Report.proxy.configured = 'unknown' }
            return
        }
    }
    if (-not $Status.available) { Add-Reason $Report 'proxy_configuration_unavailable' }
    # Record whether auto-detection was enabled at any readable check; it does
    # not establish that a proxy was discovered or used. Endpoint rules still apply.
    if ($Status.auto_detect) { $Report.proxy.auto_detect = $true }
    elseif ($Status.available -and $null -eq $Report.proxy.auto_detect) { $Report.proxy.auto_detect = $false }
    if ($Status.winhttp -or $Status.wininet -or $Status.pac) { Add-Reason $Report 'proxy_configured' }
    if ('proxy_configured' -cin $Report.reasons) { $Report.proxy.configured = 'yes' }
    elseif ('proxy_configuration_unavailable' -cin $Report.reasons) { $Report.proxy.configured = 'unknown' }
    else { $Report.proxy.configured = 'no' }
}

function Test-LoopbackPeer($Row, $TcpRows, [hashtable]$Owned) {
    if ($null -eq $Row.PSObject.Properties['LocalAddress'] -or $null -eq $Row.PSObject.Properties['LocalPort']) { return 'loopback_peer_missing' }
    $local = Get-IP ([string]$Row.LocalAddress); $remote = Get-IP ([string]$Row.RemoteAddress)
    if ($null -eq $local -or $null -eq $remote -or $Row.LocalPort -lt 1 -or $Row.LocalPort -gt 65535) { return 'loopback_peer_missing' }
    $matched = $false
    foreach ($peer in $TcpRows) {
        if ([string]$peer.State -in @('Listen', 'Bound', 'Closed')) { continue }
        if ($null -eq $peer.PSObject.Properties['LocalAddress'] -or $null -eq $peer.PSObject.Properties['LocalPort']) { continue }
        if ($peer.LocalPort -ne $Row.RemotePort -or $peer.RemotePort -ne $Row.LocalPort) { continue }
        $peerLocal = Get-IP ([string]$peer.LocalAddress); $peerRemote = Get-IP ([string]$peer.RemoteAddress)
        if ($null -eq $peerLocal -or $null -eq $peerRemote -or -not $peerLocal.Equals($remote) -or -not $peerRemote.Equals($local)) { continue }
        if (-not $Owned.ContainsKey([int]$peer.OwningProcess)) { return 'loopback_peer_foreign' }
        $matched = $true
    }
    if (-not $matched) { return 'loopback_peer_missing' }
    return 'owned'
}

function Add-DnsPair([hashtable]$Map, [string]$Name, [string]$Data) {
    $safe = Get-SafeHost $Name
    $ip = Get-IP $Data
    if ($null -eq $safe -or $null -eq $ip) { return }
    $key = $ip.ToString()
    if (-not $Map.ContainsKey($key)) { $Map[$key] = @() }
    if ($safe -cnotin $Map[$key]) { $Map[$key] += $safe }
}

function Read-LocalDns($Report, [hashtable]$Map, $State) {
    try {
        foreach ($entry in @(Get-DnsClientCache -ErrorAction Stop)) {
            if ($entry.Type -in @(1, 28) -and $entry.Status -eq 0) { Add-DnsPair $Map ([string]$entry.Entry) ([string]$entry.Data) }
        }
        $Report.dns.cache = 'readable'
    } catch { $Report.dns.cache = 'unavailable'; if ('DNS cache unavailable.' -cnotin $Report.blind_spots) { $Report.blind_spots += 'DNS cache unavailable.' } }
    if ($Report.dns.operational_log -ne 'readable') { return }
    try {
        $events = @(Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-DNS-Client/Operational'; StartTime = $State.since } -MaxEvents 512 -ErrorAction Stop)
        if ($events.Count -eq 512) { $Report.blind_spots += 'DNS event window reached the 512 event bound; correlation may be incomplete.' }
        foreach ($event in $events) {
            # Parse named XML fields only, never rendered messages. Neither raw
            # XML nor unrelated DNS names are emitted in any diagnostic/report.
            [xml]$xml = $event.ToXml()
            $fields = @{}
            foreach ($field in $xml.Event.EventData.Data) { $fields[[string]$field.Name] = [string]$field.'#text' }
            if ($fields.ContainsKey('QueryName') -and $fields.ContainsKey('QueryResults')) {
                foreach ($token in ($fields['QueryResults'] -split '[;\s]+')) { Add-DnsPair $Map $fields['QueryName'] $token }
            }
        }
        $State.since = [DateTime]::Now.AddSeconds(-1)
    } catch {
        if ($_.FullyQualifiedErrorId -notlike 'NoMatchingEventsFound*') {
            $Report.dns.operational_log = 'unavailable'
            $Report.blind_spots += 'DNS operational log became unreadable; cache-only correlation.'
        }
    }
}

function New-LiveSamplers($Report) {
    try { $Report.tools['netsh'] = (Get-Item -LiteralPath (Join-Path $env:SystemRoot 'System32/netsh.exe')).VersionInfo.FileVersion }
    catch { $Report.tools['netsh'] = 'unavailable'; Add-Reason $Report 'proxy_configuration_unavailable' }
    foreach ($name in @('Get-NetTCPConnection', 'Get-NetUDPEndpoint', 'Get-DnsClientCache', 'Get-CimInstance', 'Get-WinEvent')) {
        $command = Get-Command $name -ErrorAction SilentlyContinue
        if ($null -ne $command) { $Report.tools[$command.ModuleName] = $command.Version.ToString() }
    }
    try {
        $log = Get-WinEvent -ListLog 'Microsoft-Windows-DNS-Client/Operational' -ErrorAction Stop
        if ($log.IsEnabled) { $Report.dns.operational_log = 'readable' } else { $Report.dns.operational_log = 'disabled' }
    } catch { $Report.dns.operational_log = 'unavailable' }
    if ($Report.dns.operational_log -ne 'readable') { $Report.blind_spots += 'DNS operational log disabled/unreadable; not enabled or modified by this tool; cache-only correlation.' }
    @{
        inventory = { @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, CreationDate, Name -OperationTimeoutSec 5 -ErrorAction Stop) }
        birth = { param($id) Read-Birth $id }
        tcp = { [pscustomobject]@{ available = $true; rows = @(Get-NetTCPConnection -ErrorAction Stop) } }
        udp = { [pscustomobject]@{ available = $true; rows = @(Get-NetUDPEndpoint -ErrorAction Stop) } }
        dns = { param($r, $m, $s) Read-LocalDns $r $m $s }
        proxy = { Read-LocalProxy }
    }
}

function Read-NetworkSample($Report, [hashtable]$Known, [hashtable]$Endpoints, [hashtable]$Bindings,
    [hashtable]$DnsMap, $DnsState, [int]$RootId, [hashtable]$Samplers) {
    try { $inventory = @(& $Samplers.inventory) } catch { throw 'inventory_unavailable' }
    $owned = Read-OwnedTree $inventory $Known $RootId $Samplers.birth $Report.policy
    try { $proxyStatus = & $Samplers.proxy } catch { $proxyStatus = $null }
    Add-ProxyStatus $Report $proxyStatus
    & $Samplers.dns $Report $DnsMap $DnsState
    try { $tcp = & $Samplers.tcp } catch { throw 'tcp_unavailable' }
    try { $udp = & $Samplers.udp } catch { throw 'udp_unavailable' }
    if ($null -eq $tcp -or $null -eq $udp -or -not $tcp.available -or -not $udp.available -or
        $null -eq $tcp.rows -or $null -eq $udp.rows) { throw 'network_counters_missing' }
    # Validate identities again after the slower network samplers. A socket's
    # OwningProcess has no birth time; an intervening exit/reuse is incomplete.
    foreach ($key in @($owned.Keys)) {
        try { $born = & $Samplers.birth $key }
        catch { if ($key -eq $RootId) { throw 'root_lost' }; throw 'identity_missing' }
        if ($null -eq $born -or $born -ne $owned[$key].born) { throw 'identity_changed_during_sample' }
    }
    foreach ($row in $tcp.rows) {
        $key = [int]$row.OwningProcess
        if (-not $owned.ContainsKey($key)) { continue }
        if ([string]$row.State -in @('Listen', 'Bound', 'Closed')) { continue }
        $ip = Get-IP ([string]$row.RemoteAddress)
        if ($null -eq $ip -or $ip.Equals([Net.IPAddress]::Any) -or $ip.Equals([Net.IPAddress]::IPv6Any) -or
            $null -eq $row.RemotePort -or $row.RemotePort -lt 1 -or $row.RemotePort -gt 65535) { throw 'endpoint_invalid' }
        $address = $ip.ToString()
        $peerStatus = $null
        if ([Net.IPAddress]::IsLoopback($ip)) {
            $peerStatus = Test-LoopbackPeer $row $tcp.rows $owned
            if ($peerStatus -ne 'owned') { Add-Reason $Report $peerStatus }
        }
        $hosts = @(); if ($DnsMap.ContainsKey($address)) { $hosts = @($DnsMap[$address] | Sort-Object -Unique) }
        $endpointKey = '{0}|{1}|{2}' -f (Get-ProcessKey $key $owned[$key].born), $address, $row.RemotePort
        if (-not $Endpoints.ContainsKey($endpointKey)) {
            $Endpoints[$endpointKey] = [pscustomobject][ordered]@{
                pid = $key; born = $owned[$key].born; role = $owned[$key].role; protocol = 'tcp'
                remote_ip = $address; remote_port = [int]$row.RemotePort; hosts = @(); classification = ''; classifications = @(); loopback_peer_statuses = @(); observations = 0
            }
        }
        $endpoint = $Endpoints[$endpointKey]
        $endpoint.hosts = @(@($endpoint.hosts) + $hosts | Sort-Object -Unique)
        $endpoint.classification = Get-EndpointClass $address $endpoint.remote_port $endpoint.hosts $Report.policy $endpoint.role
        if ($endpoint.classification -cnotin $endpoint.classifications) { $endpoint.classifications += $endpoint.classification }
        $endpoint.observations++
        if ($null -ne $peerStatus -and $peerStatus -cnotin $endpoint.loopback_peer_statuses) { $endpoint.loopback_peer_statuses += $peerStatus }
        if ($endpoint.classification -like '*candidate') { Add-Reason $Report 'transport_policy_unverified' }
        if ($endpoint.classification -ceq 'runtime_host_unresolved') {
            Add-Reason $Report 'runtime_host_unresolved'
            $Report.runtime_unresolved_observations++
        }
        if ($endpoint.classification -ceq 'runtime_host_ambiguous') {
            Add-Reason $Report 'runtime_host_ambiguous'
            $Report.runtime_ambiguous_observations++
        }
        if ($endpoint.classification -ceq 'webview2_runtime_allowed') {
            $runtimeHosts = @(Get-RuntimeHostsForPort $endpoint.remote_port)
            foreach ($allowance in $Report.runtime_allowances) {
                if ($allowance.host -cin $endpoint.hosts -and $allowance.host -cin $runtimeHosts) { $allowance.observations++ }
            }
        }
    }
    foreach ($row in $udp.rows) {
        $key = [int]$row.OwningProcess
        if (-not $owned.ContainsKey($key)) { continue }
        $ip = Get-IP ([string]$row.LocalAddress)
        if ($null -eq $ip -or $null -eq $row.LocalPort -or $row.LocalPort -lt 1 -or $row.LocalPort -gt 65535) { throw 'udp_counter_invalid' }
        $bindingKey = '{0}|{1}|{2}' -f (Get-ProcessKey $key $owned[$key].born), $ip, $row.LocalPort
        if (-not $Bindings.ContainsKey($bindingKey)) {
            $Bindings[$bindingKey] = [pscustomobject][ordered]@{
                pid = $key; born = $owned[$key].born; role = $owned[$key].role; protocol = 'udp'
                local_ip = $ip.ToString(); local_port = [int]$row.LocalPort; remote = 'not_available'; observations = 0
            }
        }
        $Bindings[$bindingKey].observations++
        if (-not [Net.IPAddress]::IsLoopback($ip)) { Add-Reason $Report 'udp_remote_unobservable' }
    }
    $Report.samples++
}

function Complete-NetworkCoverage($Report, $LastStartMs, [double]$EndMs) {
    if ($null -eq $LastStartMs) {
        # No sample started: the entire collection is an uncovered gap.
        $Report.leading_coverage_gap_ms = $EndMs
        $Report.trailing_coverage_gap_ms = $EndMs
    } else {
        # Measure from the start, so a final stalled sample cannot hide the gap.
        $Report.trailing_coverage_gap_ms = $EndMs - $LastStartMs
    }
}

function Complete-NetworkReport($Report, [hashtable]$Known, [hashtable]$Endpoints, [hashtable]$Bindings) {
    $Report.processes = @($Known.Values | Sort-Object pid, born)
    $Report.endpoints = @($Endpoints.Values | Sort-Object pid, born, remote_ip, remote_port)
    $Report.udp_bindings = @($Bindings.Values | Sort-Object pid, born, local_ip, local_port)
    $Report.ended_utc = [DateTime]::UtcNow.ToString('o')
    if ($Report.samples -eq 0) { Add-Reason $Report 'no_complete_samples' }
    $Report.max_coverage_gap_ms = [Math]::Max($Report.leading_coverage_gap_ms,
        [Math]::Max($Report.max_sample_start_gap_ms, $Report.trailing_coverage_gap_ms))
    $minimumSamples = [Math]::Ceiling($Report.requested_seconds * 1000.0 / $script:CoverageGapLimitMs)
    if ($Report.max_coverage_gap_ms -gt $script:CoverageGapLimitMs -or $Report.samples -lt $minimumSamples) {
        Add-Reason $Report 'sampling_coverage_insufficient'
    }
    if ($Report.proxy.checks -eq 0) { Add-Reason $Report 'proxy_configuration_unavailable' }
    $violations = @($Report.endpoints | Where-Object { @($_.classifications | Where-Object { $_ -like '*violation' }).Count -gt 0 })
    if ($violations.Count) { $Report.result = 'fail' }
    elseif ($Report.reasons.Count) { $Report.result = 'not_measured' }
    else { $Report.result = 'pass' }
    $Report.reasons = @($Report.reasons | Sort-Object -Unique)
}

function Get-SafeFailure($Failure) {
    # Whitelist fixed reason codes; never report exception message/stack/paths.
    $code = [string]$Failure.Exception.Message
    if ($code -cin @('output_missing', 'output_reparse', 'output_not_empty', 'root_lost',
        'inventory_duplicate', 'identity_missing', 'identity_changed',
        'identity_changed_during_sample', 'network_counters_missing', 'endpoint_invalid', 'udp_counter_invalid',
        'inventory_unavailable', 'tcp_unavailable', 'udp_unavailable')) { return $code }
    return 'sampler_unavailable'
}

function Write-NetworkReport($Report, [string]$Directory) {
    $encoding = New-Object Text.UTF8Encoding($false)
    # Write the authoritative JSON last: a diagnostics failure cannot leave a
    # passing JSON behind. CreateNew protects existing evidence against races.
    $lines = @('result=' + $Report.result; 'samples=' + $Report.samples; 'endpoints=' + $Report.endpoints.Count;
        'udp_bindings=' + $Report.udp_bindings.Count; 'reasons=' + ($Report.reasons -join ',');
        'runtime_unresolved_observations=' + $Report.runtime_unresolved_observations;
        'runtime_ambiguous_observations=' + $Report.runtime_ambiguous_observations)
    foreach ($allowance in $Report.runtime_allowances) { $lines += 'runtime_allowance.' + $allowance.host + '=' + $allowance.observations }
    $bytes = $encoding.GetBytes(($lines -join "`n") + "`n")
    $stream = [IO.File]::Open((Join-Path $Directory 'diagnostics.txt'), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
    $bytes = $encoding.GetBytes(($Report | ConvertTo-Json -Depth 12) + "`n")
    $stream = [IO.File]::Open((Join-Path $Directory 'network-report.json'), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
}

function Invoke-NetworkAudit {
    $report = New-NetworkReport $Policy $Phase
    $known = @{}; $endpoints = @{}; $bindings = @{}; $dnsMap = @{}
    $dnsState = [pscustomobject]@{ since = [DateTime]::Now }
    $output = $null; $report.requested_seconds = $Seconds
    $timer = [Diagnostics.Stopwatch]::StartNew(); $lastStart = $null
    try {
        $output = Assert-NetworkOutput $OutputDir
        $samplers = New-LiveSamplers $report
        do {
            $start = $timer.Elapsed.TotalMilliseconds
            if ($null -ne $lastStart) { $report.max_sample_start_gap_ms = [Math]::Max($report.max_sample_start_gap_ms, $start - $lastStart) }
            else { $report.leading_coverage_gap_ms = $start }
            $lastStart = $start
            Read-NetworkSample $report $known $endpoints $bindings $dnsMap $dnsState $RootProcessId $samplers
            $duration = $timer.Elapsed.TotalMilliseconds - $start
            $report.max_sample_duration_ms = [Math]::Max($report.max_sample_duration_ms, $duration)
            $remaining = $Seconds * 1000 - $timer.Elapsed.TotalMilliseconds
            if ($remaining -le 0) { break }
            $sleep = [Math]::Min($remaining, [Math]::Max(1, $script:intervalMs - $duration))
            Start-Sleep -Milliseconds ([int]$sleep)
        } while ($timer.Elapsed.TotalSeconds -lt $Seconds)
        # Check the root at the end even when the final sleep consumed the window.
        try { $finalBirth = Read-Birth $RootProcessId } catch { throw 'root_lost' }
        if (-not $known.ContainsKey((Get-ProcessKey $RootProcessId $finalBirth))) { throw 'root_lost' }
    } catch { Add-Reason $report (Get-SafeFailure $_) }
    $timer.Stop()
    Complete-NetworkCoverage $report $lastStart $timer.Elapsed.TotalMilliseconds
    $report.elapsed_seconds = [Math]::Round($timer.Elapsed.TotalSeconds, 3)
    Complete-NetworkReport $report $known $endpoints $bindings
    if ($null -ne $output) {
        try { Write-NetworkReport $report $output } catch {
            Add-Reason $report 'report_write_failed'
            Complete-NetworkReport $report $known $endpoints $bindings
        }
    }
    return $report
}

function Assert-NetworkCheck([bool]$Condition, [string]$Name) {
    $script:checks++
    if (-not $Condition) { throw "FAIL: $Name" }
    Write-Output "PASS: $Name"
}

function Assert-NetworkRejected([scriptblock]$Action, [string]$Name) {
    $rejected = $false
    try { $null = & $Action } catch { $rejected = $true }
    Assert-NetworkCheck $rejected $Name
}

function Invoke-NetworkSelfTest {
    $script:checks = 0
    # Direct timing reports avoid real cmdlets, sleeps and application launches.
    foreach ($case in @(
        @{ name = 'one stalled sample covers the window'; seconds = 10; samples = 1; leading = 0; between = 0; trailing = 10000; duration = 10000; result = 'not_measured' },
        @{ name = 'leading gap exceeds limit'; seconds = 8; samples = 3; leading = 3001; between = 2000; trailing = 1000; duration = 0; result = 'not_measured' },
        @{ name = 'trailing gap exceeds limit'; seconds = 8; samples = 3; leading = 0; between = 1000; trailing = 6000; duration = 0; result = 'not_measured' },
        @{ name = 'consecutive start gap exceeds limit'; seconds = 10; samples = 4; leading = 0; between = 4000; trailing = 1000; duration = 0; result = 'not_measured' },
        @{ name = 'steady samples cover the window'; seconds = 10; samples = 10; leading = 0; between = 1000; trailing = 1000; duration = 0; result = 'pass' },
        @{ name = 'too few samples despite acceptable gaps'; seconds = 10; samples = 3; leading = 1000; between = 3000; trailing = 3000; duration = 0; result = 'not_measured' },
        @{ name = 'exact limit and minimum sample count pass'; seconds = 6; samples = 2; leading = 0; between = 3000; trailing = 3000; duration = 0; result = 'pass' }
    )) {
        $coverage = New-NetworkReport 'app' 'synthetic'
        $coverage.requested_seconds = $case.seconds; $coverage.samples = $case.samples
        $coverage.proxy.checks = 1
        $coverage.leading_coverage_gap_ms = $case.leading
        $coverage.max_sample_start_gap_ms = $case.between
        $coverage.max_sample_duration_ms = $case.duration
        $endMs = $case.seconds * 1000.0
        Complete-NetworkCoverage $coverage ($endMs - $case.trailing) $endMs
        Complete-NetworkReport $coverage @{} @{} @{}
        $expectedGap = [Math]::Max($case.leading, [Math]::Max($case.between, $case.trailing))
        Assert-NetworkCheck ($coverage.result -ceq $case.result -and $coverage.max_coverage_gap_ms -eq $expectedGap -and
            $coverage.trailing_coverage_gap_ms -eq $case.trailing -and $coverage.endpoints.Count -eq 0 -and
            (('sampling_coverage_insufficient' -cin $coverage.reasons) -eq ($case.result -ceq 'not_measured'))) $case.name
    }
    $coverage = New-NetworkReport 'app' 'synthetic'; $coverage.requested_seconds = 3
    $coverage.samples = 1; $coverage.proxy.checks = 1
    Complete-NetworkCoverage $coverage 0 4000
    Complete-NetworkReport $coverage @{} @{} @{}
    Assert-NetworkCheck ($coverage.result -ceq 'not_measured' -and $coverage.max_coverage_gap_ms -eq 4000 -and
        'sampling_coverage_insufficient' -cin $coverage.reasons) 'collection overrun counts toward trailing coverage gap'
    $coverage.samples = 3; $coverage.trailing_coverage_gap_ms = 1000
    Complete-NetworkReport $coverage @{} @{} @{}
    Assert-NetworkCheck ($coverage.result -ceq 'not_measured' -and
        'sampling_coverage_insufficient' -cin $coverage.reasons) 'insufficient coverage reason remains sticky'
    $coverageViolation = @{ observed = [pscustomobject]@{ pid = 101; born = [DateTime]::UtcNow; remote_ip = '203.0.113.1'; remote_port = 443; classifications = @('ip_only_violation') } }
    $coverage.samples = 1; $coverage.trailing_coverage_gap_ms = 4000
    Complete-NetworkReport $coverage @{} $coverageViolation @{}
    Assert-NetworkCheck ($coverage.result -ceq 'fail' -and 'sampling_coverage_insufficient' -cin $coverage.reasons) 'observed violation wins over insufficient coverage'
    $coverage = New-NetworkReport 'app' 'synthetic'; $coverage.requested_seconds = 10
    Complete-NetworkCoverage $coverage $null 10000
    Complete-NetworkReport $coverage @{} @{} @{}
    Assert-NetworkCheck ($coverage.result -ceq 'not_measured' -and $coverage.leading_coverage_gap_ms -eq 10000 -and
        $coverage.trailing_coverage_gap_ms -eq 10000 -and $coverage.max_coverage_gap_ms -eq 10000 -and
        'no_complete_samples' -cin $coverage.reasons -and 'sampling_coverage_insufficient' -cin $coverage.reasons) 'no samples retain existing reason and expose whole-window coverage gap'
    $now = [DateTime]::UtcNow.AddMinutes(-1)
    $born = $now.AddTicks(-($now.Ticks % 10))
    Assert-NetworkCheck (Test-CimBirth $born $born.AddTicks(9)) 'CIM microseconds agree with precise StartTime'
    Assert-NetworkCheck (-not (Test-CimBirth $born $born.AddTicks(10))) 'next microsecond identity mismatch rejected'
    Assert-NetworkCheck (-not (Test-CimBirth $null $born)) 'missing CIM creation date rejected'
    $script:fakeInventory = @(
        [pscustomobject]@{ ProcessId = 101; ParentProcessId = 0; CreationDate = $born; Name = 'maxed.exe' },
        [pscustomobject]@{ ProcessId = 102; ParentProcessId = 101; CreationDate = $born.AddSeconds(1); Name = 'msedgewebview2.exe' },
        [pscustomobject]@{ ProcessId = 103; ParentProcessId = 102; CreationDate = $born.AddSeconds(2); Name = 'untrusted.exe' },
        [pscustomobject]@{ ProcessId = 109; ParentProcessId = 0; CreationDate = $born; Name = 'other.exe' }
    )
    $birthReader = { param($id) ($script:fakeInventory | Where-Object { $_.ProcessId -eq $id }).CreationDate }
    $samplers = @{
        inventory = { $script:fakeInventory }; birth = $birthReader; dns = { param($r, $m, $s) }
        proxy = { [pscustomobject]@{ available = $true; winhttp = $false; wininet = $false; auto_detect = $false; pac = $false } }
        tcp = { [pscustomobject]@{ available = $true; rows = @() } }
        udp = { [pscustomobject]@{ available = $true; rows = @() } }
    }
    $known = @{}; $endpoints = @{}; $bindings = @{}; $map = @{}; $state = [pscustomobject]@{ since = $born }
    $report = New-NetworkReport 'app' 'synthetic'
    Read-NetworkSample $report $known $endpoints $bindings $map $state 101 $samplers
    Assert-NetworkCheck (($known.Values.pid | Sort-Object) -join ',' -eq '101,102,103') 'root and recursive descendants only'
    $precise = @{}
    $preciseReader = { param($id) ($script:fakeInventory | Where-Object { $_.ProcessId -eq $id }).CreationDate.AddTicks(7) }
    $null = Read-OwnedTree $script:fakeInventory $precise 101 $preciseReader 'app'
    $null = Read-OwnedTree $script:fakeInventory $precise 101 $preciseReader 'app'
    Assert-NetworkCheck ($precise.ContainsKey((Get-ProcessKey 101 $born.AddTicks(7)))) 'full StartTime retained across CIM inventories'
    $reusedReader = { param($id) ($script:fakeInventory | Where-Object { $_.ProcessId -eq $id }).CreationDate.AddTicks(8) }
    Assert-NetworkRejected { Read-OwnedTree $script:fakeInventory $precise 101 $reusedReader 'app' } 'sub-microsecond root replacement is still root loss'
    Assert-NetworkCheck ($known[(Get-ProcessKey 102 $born.AddSeconds(1))].role -eq 'webview2' -and $known[(Get-ProcessKey 103 $born.AddSeconds(2))].role -eq 'other-descendant') 'roles do not expose executable names'
    $script:fakeInventory[1].ParentProcessId = 0
    $script:fakeInventory += [pscustomobject]@{ ProcessId = 104; ParentProcessId = 102; CreationDate = $born.AddSeconds(3); Name = 'child.exe' }
    Read-NetworkSample $report $known $endpoints $bindings $map $state 101 $samplers
    Assert-NetworkCheck ($known.ContainsKey((Get-ProcessKey 104 $born.AddSeconds(3))) -and $known.ContainsKey((Get-ProcessKey 102 $born.AddSeconds(1)))) 'reparented child and its new descendants retained'
    $script:fakeInventory = @($script:fakeInventory | Where-Object { $_.ProcessId -ne 103 })
    Read-NetworkSample $report $known $endpoints $bindings $map $state 101 $samplers
    Assert-NetworkCheck $known.ContainsKey((Get-ProcessKey 103 $born.AddSeconds(2))) 'exited identities retain tombstones'
    $script:fakeInventory += [pscustomobject]@{ ProcessId = 103; ParentProcessId = 0; CreationDate = $born.AddSeconds(9); Name = 'foreign.exe' }
    $recycled = New-NetworkReport 'app' 'synthetic'
    Read-NetworkSample $recycled $known @{} @{} $map $state 101 $samplers
    Complete-NetworkReport $recycled $known @{} @{}
    Assert-NetworkCheck ($recycled.result -eq 'pass' -and -not $known.ContainsKey((Get-ProcessKey 103 $born.AddSeconds(9)))) 'retired PID reused by foreign process does not abort or become owned'
    $script:fakeInventory[-1].ParentProcessId = 101
    $active = Read-OwnedTree $script:fakeInventory $known 101 $birthReader 'app'
    Assert-NetworkCheck ($active.ContainsKey(103) -and @($known.Values | Where-Object { $_.pid -eq 103 }).Count -eq 2) 'recycled PID with valid new parent retains two separate identities'
    $reuseSamplers = $samplers.Clone()
    $reuseSamplers.tcp = { [pscustomobject]@{ available = $true; rows = @([pscustomobject]@{ OwningProcess = 103; RemoteAddress = '203.0.113.9'; RemotePort = 443; State = 'Established' }) } }
    $reuseSamplers.udp = { [pscustomobject]@{ available = $true; rows = @([pscustomobject]@{ OwningProcess = 103; LocalAddress = '127.0.0.1'; LocalPort = 12345 }) } }
    $reuseReport = New-NetworkReport 'app' 'synthetic'; $reuseEp = @{}; $reuseUdp = @{}
    Read-NetworkSample $reuseReport $known $reuseEp $reuseUdp $map $state 101 $reuseSamplers
    $script:fakeInventory[-1].CreationDate = $born.AddSeconds(10)
    Read-NetworkSample $reuseReport $known $reuseEp $reuseUdp $map $state 101 $reuseSamplers
    Assert-NetworkCheck ($reuseEp.Count -eq 2 -and $reuseUdp.Count -eq 2) 'recycled owned PID never merges TCP or UDP evidence across births'
    $script:fakeInventory += [pscustomobject]@{ ProcessId = 110; ParentProcessId = 103; CreationDate = $born.AddSeconds(5); Name = 'foreign.exe' }
    $script:fakeInventory += [pscustomobject]@{ ProcessId = 111; ParentProcessId = 101; CreationDate = $born.AddSeconds(-1); Name = 'foreign.exe' }
    $active = Read-OwnedTree $script:fakeInventory $known 101 $birthReader 'app'
    Assert-NetworkCheck (-not $active.ContainsKey(110) -and -not $active.ContainsKey(111)) 'foreign processes older than reused parent or root excluded without abort'
    $script:fakeInventory = @($script:fakeInventory | Where-Object { $_.ProcessId -notin @(110, 111) })
    $script:fakeInventory = @($script:fakeInventory | Where-Object { $_.ProcessId -ne 103 })
    $script:fakeInventory[0].CreationDate = $born.AddSeconds(10)
    Assert-NetworkRejected { Read-NetworkSample $report $known $endpoints $bindings $map $state 101 $samplers } 'reused root PID rejected'
    $script:fakeInventory[0].CreationDate = $born
    $script:fakeInventory = @($script:fakeInventory | Where-Object { $_.ProcessId -ne 101 })
    Assert-NetworkRejected { Read-NetworkSample $report $known $endpoints $bindings $map $state 101 $samplers } 'lost root rejected'
    $script:fakeInventory += [pscustomobject]@{ ProcessId = 101; ParentProcessId = 0; CreationDate = $born; Name = 'maxed.exe' }
    foreach ($selected in @('app', 'update', 'installer')) {
        foreach ($ip in @('127.0.0.1', '::1', '::ffff:127.0.0.1')) {
            Assert-NetworkCheck ((Get-EndpointClass $ip 80 @() $selected 'maxed-root') -eq 'loopback_local_allowed') "local $ip under $selected"
        }
        foreach ($ip in @('10.1.2.3', '10.0.2.2', '172.16.0.1', '172.31.255.254', '192.168.1.1', '169.254.1.1', '169.254.169.254', 'fe80::1', 'fc00::1', 'fd00::1', '::ffff:10.0.2.2')) {
            Assert-NetworkCheck ((Get-EndpointClass $ip 3128 @('github.com') $selected 'maxed-root') -eq 'local_network_violation') "LAN or metadata address $ip violates $selected even with approved DNS"
        }
        foreach ($hostName in @('evilgithub.com', 'github.com.evil.net', 'objects.githubusercontent.com', 'api.github.com', 'evil.microsoft.com')) {
            Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 443 @($hostName) $selected 'maxed-root') -like '*violation') "look-alike or unapproved host $hostName under $selected"
        }
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 443 @() $selected 'maxed-root') -eq 'ip_only_violation') "unresolved remote IP under $selected"
    }
    foreach ($hostName in @('github.com', 'release-assets.githubusercontent.com')) {
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 443 @($hostName) 'update' 'maxed-root') -eq 'approved_update_origin_candidate') "update host $hostName"
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 443 @($hostName) 'app' 'maxed-root') -like '*violation') "app blocks update host $hostName"
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 443 @($hostName) 'installer' 'maxed-root') -like '*violation') "installer blocks update host $hostName"
    }
    foreach ($hostName in @('go.microsoft.com', 'msedge.api.cdp.microsoft.com', 'msedge.sf.dl.delivery.mp.microsoft.com', 'msedge.sf.tlu.dl.delivery.mp.microsoft.com', 'msedge.sb.dl.delivery.mp.microsoft.com', 'msedge.sb.tlu.dl.delivery.mp.microsoft.com')) {
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 443 @($hostName) 'installer' 'maxed-root') -eq 'microsoft_distribution_candidate') "installer host $hostName"
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 443 @($hostName) 'app' 'maxed-root') -like '*violation') "app blocks Microsoft host $hostName"
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 443 @($hostName) 'update' 'maxed-root') -like '*violation') "update blocks Microsoft host $hostName"
    }
    foreach ($hostName in @('msedge.f.dl.delivery.mp.microsoft.com', 'msedge.f.tlu.dl.delivery.mp.microsoft.com', 'msedge.b.dl.delivery.mp.microsoft.com', 'msedge.b.tlu.dl.delivery.mp.microsoft.com')) {
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 80 @($hostName) 'installer' 'maxed-root') -eq 'microsoft_distribution_candidate') "documented HTTP installer host $hostName"
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 80 @($hostName) 'update' 'maxed-root') -like '*violation') "update blocks HTTP distribution host $hostName"
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 443 @($hostName) 'installer' 'maxed-root') -like '*violation') "Microsoft delivery scheme/host pair enforced $hostName"
    }
    Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 80 @('go.microsoft.com') 'installer' 'maxed-root') -like '*violation') 'bootstrap fwlink requires HTTPS candidate port'
    Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 80 @('github.com') 'update' 'maxed-root') -eq 'non_https_port_violation') 'port 80 rejected'
    Assert-NetworkCheck ((Get-EndpointClass '203.0.113.1' 443 @('github.com', 'evil.net') 'update' 'maxed-root') -like '*violation') 'ambiguous DNS never grants permission'
    # Exercise complete reports, including attribution and per-observation counts.
    $runtimeSamplers = $samplers.Clone()
    $runtimeSamplers.tcp = { [pscustomobject]@{ available = $true; rows = @($script:runtimeRow) } }
    $script:runtimeRow = [pscustomobject]@{ OwningProcess = 102; RemoteAddress = '203.0.113.8'; RemotePort = 443; State = 'Established' }
    foreach ($selected in @('app', 'update', 'installer')) {
        foreach ($hostName in $script:WebView2RuntimeHosts) {
            foreach ($owner in @(102, 101, 104)) {
                $script:runtimeRow.OwningProcess = $owner
                $runtimeMap = @{}; Add-DnsPair $runtimeMap ($hostName.ToUpperInvariant() + '.') '203.0.113.8'
                $runtime = New-NetworkReport $selected 'synthetic'; $runtimeEp = @{}
                Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
                Complete-NetworkReport $runtime $known $runtimeEp @{}
                if ($owner -eq 102) {
                    Assert-NetworkCheck ($runtime.result -eq 'pass' -and $runtime.endpoints[0].classification -ceq 'webview2_runtime_allowed') "runtime host $hostName allowed only for WebView2 under $selected"
                    Assert-NetworkCheck (@($runtime.runtime_allowances | Where-Object { $_.host -ceq $hostName -and $_.observations -eq 1 }).Count -eq 1) "runtime host $hostName observation counted under $selected"
                } else {
                    Assert-NetworkCheck ($runtime.result -eq 'fail' -and @($runtime.runtime_allowances | Where-Object { $_.observations -ne 0 }).Count -eq 0) "runtime host $hostName rejected for owner $owner under $selected"
                }
            }
        }
        foreach ($hostName in @('substrate.office.com.evil.net', 'evilsubstrate.office.com', 'config.edge.skype.com.attacker.org', 'github.com')) {
            Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 443 @($hostName) $selected 'webview2') -like '*violation') "WebView2 rejects $hostName under $selected"
        }
        foreach ($hostName in $script:WebView2RuntimeHosts) {
            Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 80 @($hostName) $selected 'webview2') -like '*violation') "service host $hostName still rejects port 80 under $selected"
        }
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 80 @() $selected 'webview2') -eq 'runtime_host_unresolved') "unresolved runtime port 80 is inconclusive under $selected"
        foreach ($hostName in $script:WebView2ComponentDownloadHosts) {
            foreach ($port in @(80, 443)) {
                $script:runtimeRow.OwningProcess = 102; $script:runtimeRow.RemotePort = $port
                $runtimeMap = @{}; Add-DnsPair $runtimeMap ($hostName.ToUpperInvariant() + '.') '203.0.113.8'
                $runtime = New-NetworkReport $selected 'synthetic'; $runtimeEp = @{}
                Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
                Complete-NetworkReport $runtime $known $runtimeEp @{}
                Assert-NetworkCheck ($runtime.result -eq 'pass' -and $runtime.endpoints[0].classification -ceq 'webview2_runtime_allowed') "component host $hostName port $port passes for WebView2 under $selected"
                Assert-NetworkCheck (@($runtime.runtime_allowances | Where-Object { $_.host -ceq $hostName -and $_.observations -eq 1 }).Count -eq 1) "component host $hostName port $port observation counted under $selected"
                foreach ($owner in @(101, 104)) {
                    $script:runtimeRow.OwningProcess = $owner
                    $runtime = New-NetworkReport $selected 'synthetic'; $runtimeEp = @{}
                    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
                    Complete-NetworkReport $runtime $known $runtimeEp @{}
                    # Installer roots keep the preexisting conditional delivery
                    # candidate, which is inconclusive and never a runtime pass.
                    $expected = 'fail'
                    if ($selected -eq 'installer' -and $owner -eq 101 -and $port -eq 80) { $expected = 'not_measured' }
                    Assert-NetworkCheck ($runtime.result -eq $expected -and @($runtime.runtime_allowances | Where-Object { $_.observations -ne 0 }).Count -eq 0) "owner $owner cannot inherit component allowance for $hostName port $port under $selected"
                }
            }
            Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 8080 @($hostName) $selected 'webview2') -like '*violation') "component host $hostName rejects port 8080 under $selected"
            foreach ($lookalike in @(($hostName + '.evil.net'), ('evil' + $hostName))) {
                Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 80 @($lookalike) $selected 'webview2') -like '*violation') "component look-alike $lookalike rejected under $selected"
            }
            Assert-NetworkCheck ((Get-EndpointClass '10.0.2.2' 80 @($hostName) $selected 'webview2') -eq 'local_network_violation') "component host $hostName cannot permit LAN under $selected"
        }
        $script:runtimeRow.RemotePort = 443
        foreach ($ip in @('10.0.2.2', '192.168.1.2', '169.254.1.2', 'fe80::1', 'fd00::1')) {
            Assert-NetworkCheck ((Get-EndpointClass $ip 443 @('substrate.office.com') $selected 'webview2') -eq 'local_network_violation') "runtime allowance never permits LAN $ip under $selected"
        }
        Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 443 @('github.com') $selected 'unknown') -like '*violation') "unknown role cannot inherit native delivery rules under $selected"
    }
    $script:runtimeRow.OwningProcess = 102
    $runtimeMap = @{}
    $runtime = New-NetworkReport 'app' 'synthetic'; $runtimeEp = @{}
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Complete-NetworkReport $runtime $known $runtimeEp @{}
    Assert-NetworkCheck ($runtime.result -eq 'not_measured' -and 'runtime_host_unresolved' -cin $runtime.reasons -and $runtime.runtime_unresolved_observations -eq 1) 'unresolved WebView2 external observation never passes'
    Add-DnsPair $runtimeMap 'substrate.office.com' '203.0.113.8'
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Complete-NetworkReport $runtime $known $runtimeEp @{}
    Assert-NetworkCheck ($runtime.result -eq 'not_measured' -and $runtime.runtime_unresolved_observations -eq 1 -and $runtime.runtime_allowances[0].observations -eq 1) 'later runtime DNS does not erase unresolved observation or count it as allowed'
    Add-DnsPair $runtimeMap 'outlook.office.com' '203.0.113.8'
    Add-DnsPair $runtimeMap 'outlook.office365.com' '203.0.113.8'
    $runtime = New-NetworkReport 'app' 'synthetic'; $runtimeEp = @{}
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Complete-NetworkReport $runtime $known $runtimeEp @{}
    Assert-NetworkCheck ($runtime.result -eq 'not_measured' -and 'runtime_host_ambiguous' -cin $runtime.reasons -and $runtime.runtime_ambiguous_observations -eq 2 -and $runtime.runtime_allowances[0].observations -eq 0 -and $runtime.runtime_allowances.Count -eq 7) 'substrate plus Office co-tenants is inconclusive with zero allowances'
    Assert-NetworkCheck (($runtime.endpoints[0].hosts -join ',') -ceq 'outlook.office.com,outlook.office365.com,substrate.office.com') 'all shared-IP DNS names retained alongside approved runtime name'
    Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 443 @('SUBSTRATE.OFFICE.COM.') 'app' 'webview2') -ceq 'webview2_runtime_allowed') 'runtime classification normalizes case and trailing dot'
    Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 443 @('go.microsoft.com') 'installer' 'microsoft-bootstrap-runtime') -ceq 'microsoft_distribution_candidate') 'known bootstrap role retains installer delivery candidate'
    Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 443 @('github.com') 'update' 'microsoft-bootstrap-runtime') -like '*violation') 'only Maxed root inherits GitHub update rules'
    Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 443 @('github.com') 'update') -eq 'unknown_role_violation') 'omitted role never inherits root update privileges'
    Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 443 @('github.com') 'UPDATE' 'microsoft-bootstrap-runtime') -like '*violation') 'case-insensitive policy spelling preserves root-only update rules'
    $script:runtimeRow.OwningProcess = 102; $script:runtimeRow.RemotePort = 80
    $runtime = New-NetworkReport 'app' 'synthetic'; $runtimeEp = @{}; $runtimeMap = @{}
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Complete-NetworkReport $runtime $known $runtimeEp @{}
    Assert-NetworkCheck ($runtime.result -eq 'not_measured' -and 'runtime_host_unresolved' -cin $runtime.reasons -and $runtime.runtime_unresolved_observations -eq 1) 'unresolved component port 80 never passes'
    Add-DnsPair $runtimeMap 'msedge.b.tlu.dl.delivery.mp.microsoft.com' '203.0.113.8'
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Complete-NetworkReport $runtime $known $runtimeEp @{}
    Assert-NetworkCheck ($runtime.result -eq 'not_measured' -and $runtime.runtime_unresolved_observations -eq 1 -and $runtime.runtime_allowances[3].observations -eq 1) 'later component DNS never erases unresolved observation'
    Add-DnsPair $runtimeMap 'substrate.office.com' '203.0.113.8'
    Add-DnsPair $runtimeMap 'outlook.office.com' '203.0.113.8'
    $runtime = New-NetworkReport 'app' 'synthetic'; $runtimeEp = @{}
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Complete-NetworkReport $runtime $known $runtimeEp @{}
    Assert-NetworkCheck ($runtime.result -eq 'not_measured' -and 'runtime_host_ambiguous' -cin $runtime.reasons -and $runtime.endpoints[0].hosts.Count -eq 3 -and $runtime.runtime_ambiguous_observations -eq 1 -and @($runtime.runtime_allowances | Where-Object { $_.observations -ne 0 }).Count -eq 0) 'component plus service and Office names on port 80 is inconclusive with all names retained'
    Assert-NetworkCheck ((Get-EndpointClass '203.0.113.8' 8080 @() 'app' 'webview2') -like '*violation') 'unresolved unsupported runtime port remains violation'
    $sharedCases = @(
        [pscustomobject]@{ label = 'substrate and Office co-tenant'; port = 443; names = @('substrate.office.com', 'outlook.office.com'); verdict = 'not_measured'; class = 'runtime_host_ambiguous' },
        [pscustomobject]@{ label = 'component and tracker'; port = 80; names = @('msedge.b.tlu.dl.delivery.mp.microsoft.com', 'tracker.example'); verdict = 'not_measured'; class = 'runtime_host_ambiguous' },
        [pscustomobject]@{ label = 'all HTTPS runtime names'; port = 443; names = @(Get-RuntimeHostsForPort 443); verdict = 'pass'; class = 'webview2_runtime_allowed' },
        [pscustomobject]@{ label = 'all HTTP component names'; port = 80; names = @(Get-RuntimeHostsForPort 80); verdict = 'pass'; class = 'webview2_runtime_allowed' },
        [pscustomobject]@{ label = 'service names on HTTP'; port = 80; names = @($script:WebView2RuntimeHosts); verdict = 'fail'; class = 'unapproved_host_violation' },
        [pscustomobject]@{ label = 'no approved names'; port = 443; names = @('tracker.example', 'outlook.office.com'); verdict = 'fail'; class = 'unapproved_host_violation' },
        [pscustomobject]@{ label = 'component and wrong-port service'; port = 80; names = @('msedge.b.tlu.dl.delivery.mp.microsoft.com', 'substrate.office.com'); verdict = 'not_measured'; class = 'runtime_host_ambiguous' }
    )
    foreach ($selected in @('app', 'update', 'installer')) {
        foreach ($case in $sharedCases) {
            $script:runtimeRow.RemotePort = $case.port
            $runtimeMap = @{}
            foreach ($name in $case.names) { Add-DnsPair $runtimeMap $name '203.0.113.8' }
            $runtime = New-NetworkReport $selected 'synthetic'; $runtimeEp = @{}
            Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
            Complete-NetworkReport $runtime $known $runtimeEp @{}
            Assert-NetworkCheck ($runtime.result -eq $case.verdict -and $runtime.endpoints[0].classification -eq $case.class) "$($case.label) gets $($case.verdict) under $selected"
            Assert-NetworkCheck (($runtime.endpoints[0].hosts -join ',') -ceq (($case.names | Sort-Object) -join ',')) "$($case.label) retains every correlated name under $selected"
            $expectedAllowed = 0
            if ($case.verdict -eq 'pass') { $expectedAllowed = $case.names.Count }
            Assert-NetworkCheck (@($runtime.runtime_allowances | Where-Object { $_.observations -eq 1 }).Count -eq $expectedAllowed) "$($case.label) counts only unambiguous allowed observations under $selected"
            if ($case.class -eq 'runtime_host_ambiguous') {
                Assert-NetworkCheck ('runtime_host_ambiguous' -cin $runtime.reasons -and $runtime.runtime_ambiguous_observations -eq 1) "$($case.label) has explicit ambiguity reason and count under $selected"
            }
        }
    }
    $script:runtimeRow.RemotePort = 80
    $runtimeMap = @{}; Add-DnsPair $runtimeMap 'msedge.b.tlu.dl.delivery.mp.microsoft.com' '203.0.113.8'
    $runtime = New-NetworkReport 'app' 'synthetic'; $runtimeEp = @{}
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Add-DnsPair $runtimeMap 'tracker.example' '203.0.113.8'
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Complete-NetworkReport $runtime $known $runtimeEp @{}
    Assert-NetworkCheck ($runtime.result -eq 'not_measured' -and $runtime.runtime_ambiguous_observations -eq 1 -and $runtime.runtime_allowances[3].observations -eq 1) 'new co-tenant makes earlier allowed endpoint inconclusive without counting ambiguous observation'
    $runtimeMap['203.0.113.8'] = @('msedge.b.tlu.dl.delivery.mp.microsoft.com')
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Complete-NetworkReport $runtime $known $runtimeEp @{}
    Assert-NetworkCheck ($runtime.result -eq 'not_measured' -and 'runtime_host_ambiguous' -cin $runtime.endpoints[0].classifications -and 'tracker.example' -cin $runtime.endpoints[0].hosts -and $runtime.runtime_ambiguous_observations -eq 2) 'co-tenant evidence and ambiguity stay sticky after DNS name ages out'
    $script:runtimeRow.RemoteAddress = '203.0.113.9'
    Add-DnsPair $runtimeMap 'msedge.b.tlu.dl.delivery.mp.microsoft.com' '203.0.113.9'
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Complete-NetworkReport $runtime $known $runtimeEp @{}
    Assert-NetworkCheck ($runtime.result -eq 'not_measured' -and 'runtime_host_ambiguous' -cin $runtime.reasons -and @($runtime.endpoints | Where-Object { $_.classification -eq 'webview2_runtime_allowed' }).Count -eq 1) 'ambiguity reason cannot be cleared by later clean endpoint evidence'
    $script:runtimeRow.RemoteAddress = '203.0.113.8'
    $runtimeMap = @{}; Add-DnsPair $runtimeMap 'tracker.example' '203.0.113.8'
    $runtime = New-NetworkReport 'app' 'synthetic'; $runtimeEp = @{}
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Add-DnsPair $runtimeMap 'msedge.b.tlu.dl.delivery.mp.microsoft.com' '203.0.113.8'
    Read-NetworkSample $runtime $known $runtimeEp @{} $runtimeMap $state 101 $runtimeSamplers
    Complete-NetworkReport $runtime $known $runtimeEp @{}
    Assert-NetworkCheck ($runtime.result -eq 'fail' -and 'runtime_host_ambiguous' -cin $runtime.reasons) 'later approved alias cannot downgrade an earlier observed violation'
    foreach ($canary in @('https://github.com/private/path?secret=CANARY', 'github.com?secret=CANARY', 'C:\Users\CANARY\private', '--token CANARY', 'github.com/private')) {
        Assert-NetworkCheck ($null -eq (Get-SafeHost $canary)) 'URL query/path/command-line rejected at serialization boundary'
        Add-DnsPair $map $canary '203.0.113.7'
    }
    Assert-NetworkCheck (-not $map.ContainsKey('203.0.113.7')) 'invalid DNS metadata never retained'
    Add-DnsPair $map 'GITHUB.COM.' '203.0.113.1'
    Assert-NetworkCheck ($map['203.0.113.1'][0] -ceq 'github.com') 'local DNS normalization'
    try { throw 'C:\Users\CANARY\secret?token=CANARY --command CANARY' } catch { $safe = Get-SafeFailure $_ }
    Assert-NetworkCheck ($safe -ceq 'sampler_unavailable') 'exception diagnostics use fixed codes only'
    $samplers.tcp = { [pscustomobject]@{ available = $false; rows = @() } }
    $missing = New-NetworkReport 'app' 'synthetic'
    try { Read-NetworkSample $missing $known @{} @{} $map $state 101 $samplers } catch { Add-Reason $missing (Get-SafeFailure $_) }
    Complete-NetworkReport $missing $known @{} @{}
    Assert-NetworkCheck ($missing.result -eq 'not_measured' -and 'network_counters_missing' -cin $missing.reasons) 'missing counters never pass'
    foreach ($source in @('inventory', 'tcp', 'udp')) {
        $deniedSamplers = $samplers.Clone()
        $deniedSamplers.tcp = { [pscustomobject]@{ available = $true; rows = @() } }
        $deniedSamplers[$source] = { throw 'C:\Users\CANARY\private --token CANARY' }
        $denied = New-NetworkReport 'app' 'synthetic'
        try { Read-NetworkSample $denied $known @{} @{} $map $state 101 $deniedSamplers } catch { Add-Reason $denied (Get-SafeFailure $_) }
        Complete-NetworkReport $denied $known @{} @{}
        Assert-NetworkCheck ($denied.result -eq 'not_measured' -and ($source + '_unavailable') -cin $denied.reasons) "$source read failure is explicit and cannot pass"
        Assert-NetworkCheck (($denied | ConvertTo-Json -Depth 12) -notmatch 'CANARY|C:\\Users') "$source failure diagnostics sanitized"
    }
    $empty = New-NetworkReport 'app' 'synthetic'; Complete-NetworkReport $empty @{} @{} @{}
    Assert-NetworkCheck ($empty.result -eq 'not_measured') 'zero tests/samples cannot establish pass'
    $lostSamplers = $samplers.Clone()
    $lostSamplers.inventory = { @($script:fakeInventory | Where-Object { $_.ProcessId -ne 101 }) }
    $lost = New-NetworkReport 'app' 'synthetic'; $lost.samples = 1
    try { Read-NetworkSample $lost $known @{} @{} $map $state 101 $lostSamplers } catch { Add-Reason $lost (Get-SafeFailure $_) }
    Complete-NetworkReport $lost $known @{} @{}
    Assert-NetworkCheck ($lost.result -eq 'not_measured' -and 'root_lost' -cin $lost.reasons) 'lost root invalidates an earlier clean sample'
    $samplers.tcp = { [pscustomobject]@{ available = $true; rows = @([pscustomobject]@{ OwningProcess = 101; RemoteAddress = '203.0.113.1'; RemotePort = 443; State = 'Established'; CommandLine = 'CANARY' }) } }
    $update = New-NetworkReport 'update' 'synthetic'; $ep = @{}
    Read-NetworkSample $update $known $ep @{} $map $state 101 $samplers
    Complete-NetworkReport $update $known $ep @{}
    Assert-NetworkCheck ($update.result -eq 'not_measured' -and $update.endpoints[0].role -eq 'maxed-root') 'approved root update host still requires HTTPS/path/redirect evidence'
    Assert-NetworkCheck (($update | ConvertTo-Json -Depth 12) -notmatch 'CANARY|CommandLine|https://') 'report contains no raw input fields'
    $sticky = New-NetworkReport 'update' 'synthetic'; $stickyEndpoints = @{}; $stickyMap = @{}
    Read-NetworkSample $sticky $known $stickyEndpoints @{} $stickyMap $state 101 $samplers
    Add-DnsPair $stickyMap 'github.com' '203.0.113.1'
    Read-NetworkSample $sticky $known $stickyEndpoints @{} $stickyMap $state 101 $samplers
    Complete-NetworkReport $sticky $known $stickyEndpoints @{}
    Assert-NetworkCheck ($sticky.result -eq 'fail' -and 'ip_only_violation' -cin $sticky.endpoints[0].classifications) 'later DNS candidate cannot erase observed violation'
    $app = New-NetworkReport 'app' 'synthetic'; $ep = @{}
    Read-NetworkSample $app $known $ep @{} $map $state 101 $samplers
    Add-Reason $app 'root_lost'; Complete-NetworkReport $app $known $ep @{}
    Assert-NetworkCheck ($app.result -eq 'fail') 'observed violations retained even when later data missing'
    Add-Reason $app 'report_write_failed'; Complete-NetworkReport $app $known $ep @{}
    Assert-NetworkCheck ($app.result -eq 'fail') 'report write failure cannot erase an observed violation'
    $samplers.tcp = { [pscustomobject]@{ available = $true; rows = @([pscustomobject]@{ OwningProcess = 109; RemoteAddress = '203.0.113.9'; RemotePort = 443; State = 'Established' }) } }
    $isolated = New-NetworkReport 'app' 'synthetic'; $ep = @{}
    Read-NetworkSample $isolated $known $ep @{} $map $state 101 $samplers
    Complete-NetworkReport $isolated $known $ep @{}
    Assert-NetworkCheck ($isolated.result -eq 'pass' -and $isolated.endpoints.Count -eq 0) 'unrelated OS traffic excluded'
    $script:loopbackRows = @(
        [pscustomobject]@{ OwningProcess = 102; LocalAddress = '127.0.0.1'; LocalPort = 50000; RemoteAddress = '127.0.0.1'; RemotePort = 8888; State = 'Established' },
        [pscustomobject]@{ OwningProcess = 101; LocalAddress = '127.0.0.1'; LocalPort = 8888; RemoteAddress = '127.0.0.1'; RemotePort = 50000; State = 'Established' }
    )
    $samplers.tcp = { [pscustomobject]@{ available = $true; rows = $script:loopbackRows } }
    $loopback = New-NetworkReport 'app' 'synthetic'; $ep = @{}
    Read-NetworkSample $loopback $known $ep @{} $map $state 101 $samplers
    Complete-NetworkReport $loopback $known $ep @{}
    Assert-NetworkCheck ($loopback.result -eq 'pass' -and $loopback.endpoints[0].classification -eq 'loopback_local_allowed') 'loopback TCP with mirrored owned peer passes'
    $script:loopbackRows[0].LocalAddress = '::ffff:127.0.0.1'
    Assert-NetworkCheck ((Test-LoopbackPeer $script:loopbackRows[0] $script:loopbackRows @{101=$true;102=$true}) -eq 'owned') 'mapped IPv4 mirror uses normalized addresses'
    $script:loopbackRows[0].LocalAddress = '127.0.0.1'
    $script:loopbackRows[1].OwningProcess = 109
    $foreignLoopback = New-NetworkReport 'app' 'synthetic'; $ep = @{}
    Read-NetworkSample $foreignLoopback $known $ep @{} $map $state 101 $samplers
    Complete-NetworkReport $foreignLoopback $known $ep @{}
    Assert-NetworkCheck ($foreignLoopback.result -eq 'not_measured' -and 'loopback_peer_foreign' -cin $foreignLoopback.reasons -and 'loopback_peer_foreign' -cin $foreignLoopback.endpoints[0].loopback_peer_statuses) 'loopback proxy outside owned tree cannot pass and identifies affected endpoint'
    $script:loopbackRows[1].LocalPort = 8889
    $missingPeer = New-NetworkReport 'app' 'synthetic'; $ep = @{}
    Read-NetworkSample $missingPeer $known $ep @{} $map $state 101 $samplers
    Complete-NetworkReport $missingPeer $known $ep @{}
    Assert-NetworkCheck ($missingPeer.result -eq 'not_measured' -and 'loopback_peer_missing' -cin $missingPeer.reasons) 'nonmatching mirror cannot prove loopback peer ownership'
    $script:loopbackRows[1].LocalPort = 8888; $script:loopbackRows[1].OwningProcess = 101
    $lanSamplers = $samplers.Clone()
    $lanSamplers.tcp = { [pscustomobject]@{ available = $true; rows = @([pscustomobject]@{ OwningProcess = 102; RemoteAddress = '10.0.2.2'; RemotePort = 3128; State = 'Established' }) } }
    $lanReport = New-NetworkReport 'app' 'synthetic'; $ep = @{}
    Read-NetworkSample $lanReport $known $ep @{} $map $state 101 $lanSamplers
    Complete-NetworkReport $lanReport $known $ep @{}
    Assert-NetworkCheck ($lanReport.result -eq 'fail' -and $lanReport.endpoints[0].classification -eq 'local_network_violation') 'LAN proxy destination fails full report pipeline'
    foreach ($field in @('winhttp', 'wininet', 'pac')) {
        $proxyReport = New-NetworkReport 'app' 'synthetic'; $proxyReport.samples = 1
        $status = & $samplers.proxy; $status.$field = $true
        Add-ProxyStatus $proxyReport $status
        Complete-NetworkReport $proxyReport @{} @{} @{}
        Assert-NetworkCheck ($proxyReport.result -eq 'not_measured' -and $proxyReport.proxy.configured -eq 'yes' -and 'proxy_configured' -cin $proxyReport.reasons) "$field proxy configuration prevents pass"
        Add-ProxyStatus $proxyReport (& $samplers.proxy)
        Complete-NetworkReport $proxyReport @{} @{} @{}
        Assert-NetworkCheck ($proxyReport.result -eq 'not_measured' -and $proxyReport.proxy.configured -eq 'yes') "$field proxy configuration evidence is sticky"
    }
    $proxySamplers = $samplers.Clone()
    $proxySamplers.proxy = { [pscustomobject]@{ available = $true; winhttp = $false; wininet = $false; auto_detect = $true; pac = $false } }
    $proxyPipeline = New-NetworkReport 'app' 'synthetic'; $ep = @{}
    Read-NetworkSample $proxyPipeline $known $ep @{} $map $state 101 $proxySamplers
    Complete-NetworkReport $proxyPipeline $known $ep @{}
    Assert-NetworkCheck ($proxyPipeline.result -eq 'pass' -and $proxyPipeline.proxy.auto_detect -eq $true -and $proxyPipeline.proxy.configured -eq 'no' -and 'proxy_configured' -cnotin $proxyPipeline.reasons) 'auto-detect alone allows owned loopback traffic and records the flag'
    Add-ProxyStatus $proxyPipeline (& $samplers.proxy)
    Complete-NetworkReport $proxyPipeline $known $ep @{}
    Assert-NetworkCheck ($proxyPipeline.result -eq 'pass' -and $proxyPipeline.proxy.auto_detect -eq $true) 'auto-detect observation is retained without becoming a blocking reason'
    $autoSamplers = $proxySamplers.Clone()
    $autoSamplers.tcp = { [pscustomobject]@{ available = $true; rows = @() } }
    foreach ($selected in @('app', 'update', 'installer')) {
        $autoClean = New-NetworkReport $selected 'synthetic'
        Read-NetworkSample $autoClean $known @{} @{} $map $state 101 $autoSamplers
        Complete-NetworkReport $autoClean $known @{} @{}
        Assert-NetworkCheck ($autoClean.result -eq 'pass' -and $autoClean.proxy.auto_detect -eq $true -and $autoClean.reasons.Count -eq 0) "auto-detect with clean endpoints passes $selected"
    }
    $autoSamplers.tcp = $lanSamplers.tcp
    foreach ($selected in @('app', 'update')) {
        $autoLan = New-NetworkReport $selected 'synthetic'; $ep = @{}
        Read-NetworkSample $autoLan $known $ep @{} $map $state 101 $autoSamplers
        Complete-NetworkReport $autoLan $known $ep @{}
        Assert-NetworkCheck ($autoLan.result -eq 'fail' -and $autoLan.endpoints[0].classification -eq 'local_network_violation') "auto-detect with discovered LAN proxy fails $selected"
    }
    $autoSamplers.tcp = { [pscustomobject]@{ available = $true; rows = @([pscustomobject]@{ OwningProcess = 102; RemoteAddress = '203.0.113.19'; RemotePort = 443; State = 'Established' }) } }
    $autoPublic = New-NetworkReport 'update' 'synthetic'; $ep = @{}; $proxyDns = @{'203.0.113.19' = @('proxy.invalid')}
    Read-NetworkSample $autoPublic $known $ep @{} $proxyDns $state 101 $autoSamplers
    Complete-NetworkReport $autoPublic $known $ep @{}
    Assert-NetworkCheck ($autoPublic.result -eq 'fail' -and $autoPublic.endpoints[0].classification -eq 'unapproved_host_violation') 'discovered public proxy is a violation under update policy'
    $proxySamplers.proxy = { throw 'CANARY https://proxy.invalid/private?token=CANARY' }
    $unreadableProxy = New-NetworkReport 'app' 'synthetic'; $ep = @{}
    Read-NetworkSample $unreadableProxy $known $ep @{} $map $state 101 $proxySamplers
    Complete-NetworkReport $unreadableProxy $known $ep @{}
    Assert-NetworkCheck ($unreadableProxy.result -eq 'not_measured' -and $unreadableProxy.proxy.configured -eq 'unknown') 'unreadable proxy configuration cannot pass'
    Assert-NetworkCheck (($unreadableProxy | ConvertTo-Json -Depth 12) -notmatch 'CANARY|proxy.invalid') 'proxy reader exception text never serialized'
    $unprobedProxy = New-NetworkReport 'app' 'synthetic'; $unprobedProxy.samples = 1
    Complete-NetworkReport $unprobedProxy @{} @{} @{}
    Assert-NetworkCheck ($unprobedProxy.result -eq 'not_measured') 'complete counters without a proxy probe cannot pass'
    $partialProxy = New-NetworkReport 'app' 'synthetic'; $partialProxy.samples = 1
    Add-ProxyStatus $partialProxy ([pscustomobject]@{ available = $false; winhttp = $true; wininet = $false; auto_detect = $false; pac = $false })
    Complete-NetworkReport $partialProxy @{} @{} @{}
    Assert-NetworkCheck ($partialProxy.result -eq 'not_measured' -and $partialProxy.proxy.configured -eq 'yes' -and 'proxy_configuration_unavailable' -cin $partialProxy.reasons) 'partial proxy probe preserves known configured status without passing'
    foreach ($fixture in @(
        '{"ProxyIsEnabled":true,"AutoConfigIsEnabled":false,"AutoDetect":false,"Proxy":"https://CANARY/private?token=CANARY"}',
        '{"Proxy":"https://CANARY/private?token=CANARY","AutoconfigUrl":"","AutoDetect":false}'
    )) {
        $flags = ConvertFrom-AdvancedProxy $fixture
        Assert-NetworkCheck ($flags.manual -and -not $flags.pac -and -not $flags.auto_detect) 'advanced proxy flags parsed without retaining addresses'
        Assert-NetworkCheck (($flags | ConvertTo-Json) -notmatch 'CANARY|https|token') 'advanced proxy URL and query redacted by omission'
    }
    Assert-NetworkRejected { ConvertFrom-AdvancedProxy '{"ProxyIsEnabled":"false","AutoConfigIsEnabled":false,"AutoDetect":false}' } 'invalid proxy flag type cannot imply direct access'
    Assert-NetworkRejected { ConvertFrom-AdvancedProxy 'unavailable CANARY' } 'missing advanced configuration cannot imply direct access'
    $samplers.udp = { [pscustomobject]@{ available = $false; rows = @() } }
    $missingUdp = New-NetworkReport 'app' 'synthetic'
    try { Read-NetworkSample $missingUdp $known @{} @{} $map $state 101 $samplers } catch { Add-Reason $missingUdp (Get-SafeFailure $_) }
    Complete-NetworkReport $missingUdp $known @{} @{}
    Assert-NetworkCheck ($missingUdp.result -eq 'not_measured') 'missing UDP counters never pass'
    $samplers.udp = { [pscustomobject]@{ available = $true; rows = @([pscustomobject]@{ OwningProcess = 102; LocalAddress = '0.0.0.0'; LocalPort = 12345 }) } }
    $quic = New-NetworkReport 'app' 'synthetic'; $udp = @{}
    Read-NetworkSample $quic $known @{} $udp $map $state 101 $samplers
    Complete-NetworkReport $quic $known @{} $udp
    Assert-NetworkCheck ($quic.result -eq 'not_measured' -and $quic.udp_bindings[0].remote -eq 'not_available') 'UDP wildcard is unknown outbound, never zero'
    $samplers.udp = { [pscustomobject]@{ available = $true; rows = @() } }
    $script:birthCalls = 0
    $script:fakeBirthReader = $birthReader
    $samplers.birth = { param($id) $script:birthCalls++; if ($script:birthCalls -gt 4) { return [DateTime]::UtcNow }; & $script:fakeBirthReader $id }
    $midSample = New-NetworkReport 'app' 'synthetic'
    try { Read-NetworkSample $midSample $known @{} @{} $map $state 101 $samplers } catch { Add-Reason $midSample (Get-SafeFailure $_) }
    Complete-NetworkReport $midSample $known @{} @{}
    Assert-NetworkCheck ($midSample.result -eq 'not_measured' -and 'identity_changed_during_sample' -cin $midSample.reasons) 'mid-sample PID reuse still invalidates socket attribution'
    $script:fakeBirthReader = $birthReader
    $samplers.birth = { param($id) if ($id -eq 102) { return $script:fakeInventory[0].CreationDate.AddSeconds(20) }; & $script:fakeBirthReader $id }
    Assert-NetworkRejected { Read-NetworkSample $report $known @{} @{} $map $state 101 $samplers } 'process counter identity mismatch'
    $temp = Join-Path $PSScriptRoot ('network-selftest-' + [Guid]::NewGuid().ToString('N'))
    $null = New-Item -ItemType Directory -Path $temp
    try {
        Assert-NetworkCheck ((Assert-NetworkOutput $temp) -ceq $temp) 'existing empty output accepted'
        Write-NetworkReport $isolated $temp
        $diagnostics = @(Get-Content -LiteralPath (Join-Path $temp 'diagnostics.txt'))
        Assert-NetworkCheck ($diagnostics.Count -eq 14 -and $diagnostics[0] -ceq 'result=pass' -and $diagnostics[1] -ceq 'samples=1' -and 'runtime_allowance.substrate.office.com=0' -cin $diagnostics -and 'runtime_allowance.msedge.b.tlu.dl.delivery.mp.microsoft.com=0' -cin $diagnostics -and 'runtime_unresolved_observations=0' -cin $diagnostics -and 'runtime_ambiguous_observations=0' -cin $diagnostics) 'diagnostic and runtime counts are separate readable lines'
        Assert-NetworkRejected { Write-NetworkReport $isolated $temp } 'CreateNew report prevents concurrent overwrite'
        Assert-NetworkRejected { Assert-NetworkOutput $temp } 'nonempty output rejected including previous evidence'
        Assert-NetworkRejected { Assert-NetworkOutput (Join-Path $temp 'absent') } 'missing output rejected'
        $json = [IO.File]::ReadAllText((Join-Path $temp 'network-report.json'))
        Assert-NetworkCheck ($json -notmatch 'CANARY|C:\\Users|CommandLine') 'written evidence sanitized'
        $reparse = Join-Path $temp 'link'
        $null = New-Item -ItemType Junction -Path $reparse -Target $temp
        try { Assert-NetworkRejected { Assert-NetworkOutput $reparse } 'reparse output rejected' }
        finally { [IO.Directory]::Delete($reparse) }
    } finally {
        # Fixed owned fixture root, validated before recursive deletion.
        $resolved = [IO.Path]::GetFullPath($temp)
        if (-not $resolved.StartsWith([IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'cleanup_refused' }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    if ($script:checks -eq 0) { throw 'No self-tests discovered' }
    Write-Output "Network audit: $script:checks checks passed. Mock samplers only; no network or app launch."
}

if ($SelfTest) {
    try { Invoke-NetworkSelfTest; exit 0 } catch { Write-Output "Network audit self-test failed at check $script:checks."; exit 1 }
}
$result = Invoke-NetworkAudit
Write-Output ($result | ConvertTo-Json -Depth 12)
if ($result.result -ne 'pass') { exit 1 }
exit 0
