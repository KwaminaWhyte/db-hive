# Code Signing Guide

This document explains how to set up code signing for DB-Hive across different platforms.

## Linux - AppImage Signing with GPG

AppImage packages can be signed using GPG to allow users to verify the authenticity of the downloaded file.

### Prerequisites

Install GPG if not already installed:
```bash
# Ubuntu/Debian
sudo apt install gnupg

# Fedora
sudo dnf install gnupg
```

### 1. Generate a GPG Key

Generate a new GPG key for signing:

```bash
gpg --full-gen-key
```

Follow the prompts:
- Choose **RSA and RSA** (default)
- Key size: **4096** bits (recommended for signing)
- Expiration: Choose based on your needs (e.g., 2 years)
- Real name: **DB-Hive Release** (or your organization name)
- Email: Use your official email
- Comment: **Code Signing Key** (optional)

### 2. List Your Keys

```bash
gpg --list-secret-keys --keyid-format LONG
```

Output example:
```
/home/user/.gnupg/pubring.kbx
------------------------------------
sec   rsa4096/ABCD1234EFGH5678 2025-11-19 [SC]
      1234567890ABCDEF1234567890ABCDEF12345678
uid                 [ultimate] DB-Hive Release <release@example.com>
ssb   rsa4096/WXYZ9876STUV4321 2025-11-19 [E]
```

The key ID is `ABCD1234EFGH5678` (after `rsa4096/`).

### 3. Export Your Public Key

Export the public key for distribution:

```bash
gpg --armor --export ABCD1234EFGH5678 > db-hive-public-key.asc
```

**Important:** Publish this public key on your website, GitHub releases, or keyserver so users can verify signatures.

### 4. Sign AppImage Locally

To sign the AppImage during local builds:

```bash
# Set environment variables
export SIGN=1
export SIGN_KEY=ABCD1234EFGH5678
export APPIMAGETOOL_SIGN_PASSPHRASE="your-gpg-passphrase"
export APPIMAGETOOL_FORCE_SIGN=1  # Exit on signing errors

# Build with signing
npm run tauri build
```

### 5. Verify Signature

Users can verify the signature using the AppImage validate tool:

1. Download validate tool:
```bash
wget https://github.com/AppImageCommunity/AppImageUpdate/releases/download/continuous/validate-x86_64.AppImage
chmod +x validate-x86_64.AppImage
```

2. Import your public key:
```bash
gpg --import db-hive-public-key.asc
```

3. Validate the AppImage:
```bash
./validate-x86_64.AppImage db-hive_0.1.0_amd64.AppImage
```

Expected output:
```
Validation result: validation successful
Signatures found with key fingerprints: ABCD1234EFGH5678
```

### 6. GitHub Actions Setup

To sign AppImages in GitHub Actions:

1. Export your private key:
```bash
gpg --armor --export-secret-keys ABCD1234EFGH5678 > private-key.asc
```

2. Add to GitHub Secrets:
   - Go to repository Settings → Secrets and variables → Actions
   - Add secrets:
     - `GPG_PRIVATE_KEY`: Content of `private-key.asc`
     - `GPG_PASSPHRASE`: Your GPG key passphrase
     - `GPG_KEY_ID`: Your key ID (e.g., `ABCD1234EFGH5678`)

3. Update workflow (already configured in `.github/workflows/release.yml`):
```yaml
- name: Import GPG key
  if: matrix.platform == 'ubuntu-22.04'
  run: |
    echo "${{ secrets.GPG_PRIVATE_KEY }}" | gpg --import

- name: Build and release Tauri app
  uses: tauri-apps/tauri-action@v0
  env:
    GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
    SIGN: 1
    SIGN_KEY: ${{ secrets.GPG_KEY_ID }}
    APPIMAGETOOL_SIGN_PASSPHRASE: ${{ secrets.GPG_PASSPHRASE }}
    APPIMAGETOOL_FORCE_SIGN: 1
```

**Security Note:** Delete `private-key.asc` after adding to GitHub Secrets!

---

## macOS - Code Signing

macOS code signing requires an Apple Developer account and certificates.

### For Local Development

1. **Join Apple Developer Program** ($99/year)
2. **Create Developer ID Certificate** in Xcode or developer portal
3. **Set up in Xcode:**
   - Xcode → Settings → Accounts
   - Add your Apple ID
   - Download certificates

### For GitHub Actions

See official Tauri documentation:
- [macOS Code Signing Guide](https://v2.tauri.app/distribute/sign/macos/)

You'll need to:
1. Export your certificate and private key as `.p12`
2. Add to GitHub Secrets:
   - `APPLE_CERTIFICATE`: Base64-encoded `.p12` file
   - `APPLE_CERTIFICATE_PASSWORD`: Certificate password
   - `APPLE_ID`: Your Apple ID email
   - `APPLE_PASSWORD`: App-specific password
   - `APPLE_TEAM_ID`: Your team ID

---

## Windows - Code Signing

Windows code signing requires a code signing certificate from a trusted Certificate Authority.

### Certificate Options

1. **Extended Validation (EV) Certificate** (Recommended)
   - No SmartScreen warnings for new apps
   - USB token required
   - Cost: ~$300-$500/year

2. **Standard Code Signing Certificate**
   - May trigger SmartScreen initially
   - Build reputation over time
   - Cost: ~$100-$200/year

### Providers

- DigiCert
- Sectigo (formerly Comodo)
- SSL.com

### For GitHub Actions

See official Tauri documentation:
- [Windows Code Signing Guide](https://v2.tauri.app/distribute/sign/windows/)

You'll need to add GitHub Secrets:
- `WINDOWS_CERTIFICATE`: Base64-encoded certificate
- `WINDOWS_CERTIFICATE_PASSWORD`: Certificate password

---

## Important Notes

### Security Best Practices

1. **Never commit private keys** to version control
2. **Use strong passphrases** for all keys
3. **Rotate keys periodically** (every 1-2 years)
4. **Back up keys securely** (encrypted external drive, password manager)
5. **Publish public keys** on authenticated channels (HTTPS website, verified GitHub)

### Cross-Platform Matrix

| Platform | Signing Method | Cost | Required For |
|----------|---------------|------|--------------|
| Linux    | GPG          | Free | User trust (optional) |
| macOS    | Apple Dev ID | $99/year | Notarization, Gatekeeper |
| Windows  | Code Cert    | $100-$500/year | SmartScreen trust |

### Release Checklist

- [ ] Linux: AppImage signed with GPG
- [ ] macOS: App signed and notarized with Developer ID
- [ ] Windows: Installer signed with code signing certificate
- [ ] Public keys/certificates published
- [ ] Release notes include verification instructions
- [ ] SHA256 checksums generated for all artifacts

---

## References

- [Tauri Signing Documentation](https://v2.tauri.app/distribute/sign/)
- [AppImage Signature Documentation](https://docs.appimage.org/packaging-guide/optional/signatures.html)
- [GPG Documentation](https://gnupg.org/documentation/)
