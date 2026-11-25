param([Parameter(Mandatory)][string]$Installer)
$ErrorActionPreference = 'Stop'

if (!('PocketCamInstallerCrc' -as [type])) {
    Add-Type -TypeDefinition @'
using System;

public static class PocketCamInstallerCrc {
    public static uint Calculate(byte[] executable, int start, int end) {
        uint[] table = new uint[256];
        for (uint i = 0; i < table.Length; i++) {
            uint entry = i;
            for (int bit = 0; bit < 8; bit++)
                entry = (entry >> 1) ^ ((entry & 1) == 1 ? 0xEDB88320u : 0);
            table[i] = entry;
        }
        uint crc = 0xFFFFFFFF;
        for (int i = start; i < end; i++)
            crc = (crc >> 8) ^ table[(crc ^ executable[i]) & 0xFF];
        return ~crc;
    }
}
'@
}

$file = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Installer).Path)
if ($file.Length -le 516 -or $file[0] -ne 0x4D -or $file[1] -ne 0x5A) {
    throw 'The setup file is not a Windows installer.'
}
# For an unsigned NSIS installer the first 512 bytes are skipped and the CRC is the trailer.
$end = $file.Length - 4
$expected = [BitConverter]::ToUInt32($file, $end)
$actual = [PocketCamInstallerCrc]::Calculate($file, 512, $end)
if ($actual -ne $expected) {
    throw ('Installer integrity check failed (expected {0:X8}, calculated {1:X8}).' -f $expected, $actual)
}
Write-Host ('Installer integrity verified ({0:X8}).' -f $actual)
