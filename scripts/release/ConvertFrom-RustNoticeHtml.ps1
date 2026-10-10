function ConvertFrom-RustNoticeHtml([string]$Html) {
    # This is a renderer for the reviewed, pinned Rust document, not arbitrary
    # HTML. Its tags are XML-compatible except for the doctype and void meta tag.
    # Parse entities once, without a browser, network access or platform HTML DOM.
    $xml = $Html.Replace('<!DOCTYPE html>', '').Replace('<meta charset="UTF-8">', '<meta charset="UTF-8" />')
    $settings = New-Object Xml.XmlReaderSettings
    $settings.DtdProcessing = [Xml.DtdProcessing]::Prohibit
    $settings.XmlResolver = $null
    $reader = [Xml.XmlReader]::Create((New-Object IO.StringReader($xml)), $settings)
    $document = New-Object Xml.XmlDocument
    $document.PreserveWhitespace = $true
    $document.XmlResolver = $null
    try { $document.Load($reader) } finally { $reader.Dispose() }
    if ($document.DocumentElement.Name -cne 'html' -or $null -eq $document.html.body) {
        throw 'Unsupported Rust notice HTML: expected html/body.'
    }
    $rendered = New-Object Text.StringBuilder
    function Break-RustNoticeLine {
        param([switch]$PreserveTrailing)
        if (-not $PreserveTrailing) {
            while ($rendered.Length -and $rendered[$rendered.Length - 1] -eq ' ') { $rendered.Length-- }
        }
        if ($rendered.Length -and $rendered[$rendered.Length - 1] -ne "`n") { $null = $rendered.Append("`n") }
    }
    function Write-RustNoticeNode($Node, [int]$Depth) {
        if ($Node.NodeType -in @([Xml.XmlNodeType]::Text, [Xml.XmlNodeType]::Whitespace, [Xml.XmlNodeType]::SignificantWhitespace)) {
            # HTML flow whitespace is formatting; preformatted notices below
            # retain every decoded character, including spaces and blank lines.
            if ([string]::IsNullOrWhiteSpace($Node.Value)) {
                if ($rendered.Length -and $rendered[$rendered.Length - 1] -cnotin @("`n", ' ')) { $null = $rendered.Append(' ') }
                return
            }
            $text = [regex]::Replace($Node.Value, '[ \t\r\n]+', ' ')
            if (-not $rendered.Length -or $rendered[$rendered.Length - 1] -eq "`n") {
                $null = $rendered.Append('  ' * $Depth)
                $text = $text.TrimStart(' ')
            }
            $null = $rendered.Append($text)
            return
        }
        if ($Node.NodeType -ne [Xml.XmlNodeType]::Element -or
            $Node.Name -cnotin @('body', 'h1', 'h2', 'h3', 'p', 'ul', 'li', 'div', 'details', 'summary', 'pre', 'a', 'b', 'code')) {
            throw "Unsupported Rust notice HTML node: $($Node.Name)."
        }
        if ($Node.Name -ceq 'pre') {
            # Fail closed if a future source nests markup inside a licence text.
            if (@($Node.ChildNodes | Where-Object NodeType -eq ([Xml.XmlNodeType]::Element)).Count) {
                throw 'Unsupported Rust notice HTML: markup inside pre.'
            }
            Break-RustNoticeLine
            $null = $rendered.Append($Node.InnerText.Replace("`r`n", "`n").Replace("`r", "`n"))
            Break-RustNoticeLine -PreserveTrailing
            return
        }
        $block = $Node.Name -cin @('body', 'h1', 'h2', 'h3', 'p', 'ul', 'li', 'div', 'details', 'summary')
        if ($block) { Break-RustNoticeLine }
        # Nested divs are the in-tree file/directory exception hierarchy.
        $childDepth = if ($Node.Name -ceq 'div') { $Depth + 1 } else { $Depth }
        foreach ($child in $Node.ChildNodes) { Write-RustNoticeNode $child $childDepth }
        if ($Node.Name -ceq 'a') {
            $href = $Node.GetAttribute('href')
            if ($href -cmatch '^https?://') { $null = $rendered.Append(" ($href)") }
            elseif (-not $href.StartsWith('#', [StringComparison]::Ordinal)) { throw 'Unsupported Rust notice HTML link.' }
        }
        if ($block) { Break-RustNoticeLine }
    }
    Write-RustNoticeNode $document.html.body 0
    $rendered.ToString()
}
