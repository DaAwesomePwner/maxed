# Embedded verbatim in release.yml: actions: read deliberately cannot check out private source.
$tested = 'false'
try {
  for ($page = 1; $page -le 10; $page++) {
    $response = Invoke-RestMethod -Uri "$env:GITHUB_API_URL/repos/$env:GITHUB_REPOSITORY/actions/workflows/tests.yml/runs?head_sha=$env:GITHUB_SHA&per_page=100&page=$page" -Headers @{ Authorization = "Bearer $env:GH_TOKEN"; Accept = 'application/vnd.github+json' } -ErrorAction Stop
    $match = @($response.workflow_runs | Where-Object { $_.path -ceq '.github/workflows/tests.yml' -and $_.head_repository.full_name -ceq 'daplabs/maxed' -and $_.head_sha -ceq $env:GITHUB_SHA -and $_.head_branch -ceq 'main' -and $_.event -cin @('push', 'schedule', 'workflow_dispatch') -and $_.status -ceq 'completed' -and $_.conclusion -ceq 'success' })
    if ($match.Count) { "Reused tests: $($match[0].html_url)" >> $env:GITHUB_STEP_SUMMARY; $tested = 'true'; break }
    if (@($response.workflow_runs).Count -lt 100) { break }
  }
} catch { Write-Warning 'Prior tests lookup failed; running tests.'; $tested = 'false' }
"tested=$tested" >> $env:GITHUB_OUTPUT
