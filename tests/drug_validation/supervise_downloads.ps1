# Auto-supervise aria2 downloads: restart on stall until complete
$aria = "d:\Project\rust\GenoLigand\.tools\aria2\bin\aria2-1.37.0-win-64bit-build1\aria2c.exe"

$tasks = @(
    @{
        Name = "chembl"
        File = "D:\GenoLigandData\science_store\downloads\chembl\chembl_sqlite.tar.gz"
        Dir  = "D:\GenoLigandData\science_store\downloads\chembl"
        Out  = "chembl_sqlite.tar.gz"
        Url  = "https://ftp.ebi.ac.uk/pub/databases/chembl/ChEMBLdb/releases/chembl_37/chembl_37_sqlite.tar.gz"
        Total = 5764252857
        Proxy = "http://127.0.0.1:7897"
    },
    @{
        Name = "bindingdb"
        File = "D:\GenoLigandData\science_store\downloads\bindingdb\bindingdb_all.tsv.zip"
        Dir  = "D:\GenoLigandData\science_store\downloads\bindingdb"
        Out  = "bindingdb_all.tsv.zip"
        Url  = "https://www.bindingdb.org/rwd/bind/downloads/BindingDB_All_202607_tsv.zip"
        Total = 601012750
        Proxy = ""
    }
)

function Start-OneDownload($t) {
    if ($t.Proxy) {
        $proc = Start-Process -FilePath $aria -ArgumentList @(
            "--all-proxy=$($t.Proxy)", "-x", "16", "-s", "16", "-k", "4M",
            "--file-allocation=none", "--continue=true", "--max-tries=0",
            "--retry-wait=5", "--console-log-level=warn", "--summary-interval=60",
            "-d", $t.Dir, "-o", $t.Out, $t.Url
        ) -WindowStyle Hidden -PassThru
    } else {
        $proc = Start-Process -FilePath $aria -ArgumentList @(
            "-x", "16", "-s", "16", "-k", "4M",
            "--file-allocation=none", "--continue=true", "--max-tries=0",
            "--retry-wait=5", "--console-log-level=warn", "--summary-interval=60",
            "-d", $t.Dir, "-o", $t.Out, $t.Url
        ) -WindowStyle Hidden -PassThru
    }
    return $proc.Id
}

function Stop-OneDownload($procId) {
    Stop-Process -Id $procId -Force -ErrorAction SilentlyContinue
}

# Start both initially
$taskPids = @{}
foreach ($t in $tasks) { $taskPids[$t.Name] = Start-OneDownload $t }
Write-Host "[$(Get-Date -Format HH:mm:ss)] Started both aria2 downloads (PIDs: $($taskPids['chembl']), $($taskPids['bindingdb']))"

$lastSizes = @{}
foreach ($t in $tasks) { $lastSizes[$t.Name] = (Get-Item $t.File).Length }
$stallCounter = @{}
foreach ($t in $tasks) { $stallCounter[$t.Name] = 0 }

while ($true) {
    Start-Sleep -Seconds 45
    $allDone = $true
    foreach ($t in $tasks) {
        $size = (Get-Item $t.File).Length
        $pct = [math]::Round($size / $t.Total * 100, 2)
        $delta = $size - $lastSizes[$t.Name]

        if ($size -ge $t.Total) {
            Write-Host "[$(Get-Date -Format HH:mm:ss)] $($t.Name): COMPLETE ($([math]::Round($size/1MB,0)) MB)"
            # Kill aria2 processes for this completed file (they exit on their own usually)
            continue
        }
        $allDone = $false

        if ($delta -lt 1048576) {  # less than 1MB in 45s = stalled
            $stallCounter[$t.Name]++
            Write-Host "[$(Get-Date -Format HH:mm:ss)] $($t.Name): $([math]::Round($size/1MB,0))MB ($pct%) stalled x$($stallCounter[$t.Name])"
            if ($stallCounter[$t.Name] -ge 3) {  # stalled for ~2.25 min
                Write-Host "[$(Get-Date -Format HH:mm:ss)] $($t.Name): restarting aria2..."
                Stop-OneDownload $taskPids[$t.Name]
                Start-Sleep -Seconds 2
                $taskPids[$t.Name] = Start-OneDownload $t
                $stallCounter[$t.Name] = 0
            }
        } else {
            $stallCounter[$t.Name] = 0
            Write-Host "[$(Get-Date -Format HH:mm:ss)] $($t.Name): $([math]::Round($size/1MB,0))MB ($pct%) +$([math]::Round($delta/1MB,1))MB/45s"
        }
        $lastSizes[$t.Name] = $size
    }
    if ($allDone) {
        Write-Host "[$(Get-Date -Format HH:mm:ss)] ALL DOWNLOADS COMPLETE"
        break
    }
}
