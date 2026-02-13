import { FC, useState, useEffect, FormEvent, ChangeEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Eye, EyeOff, Key } from "lucide-react";
import {
  ConnectionProfile,
  DbDriver,
  SslMode,
  SshConfig,
  SshAuthMethod,
  ConnectionStatus,
  getDefaultPort,
  getDriverDisplayName,
} from "../types/database";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface ConnectionFormProps {
  driver: DbDriver;
  profile?: ConnectionProfile;
  onSuccess?: (profileId: string) => void;
}

export const ConnectionForm: FC<ConnectionFormProps> = ({
  driver,
  profile,
  onSuccess,
}) => {
  // Form state
  const [formData, setFormData] = useState<Partial<ConnectionProfile>>({
    id: profile?.id || "",
    name: profile?.name || "",
    driver: driver,
    host: profile?.host || "localhost",
    port: profile?.port || getDefaultPort(driver),
    username: profile?.username || "",
    database: profile?.database || "",
    sslMode: profile?.sslMode || "Disable",
  });

  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [enableKeychain, setEnableKeychain] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [testStatus, setTestStatus] = useState<string | null>(null);

  // Connection string
  const [connectionString, setConnectionString] = useState("");

  // SSH Tunnel state
  const [sshMode, setSshMode] = useState<"off" | "ssh">(
    profile?.sshTunnel ? "ssh" : "off"
  );
  const [sshConfig, setSshConfig] = useState<Partial<SshConfig>>({
    host: profile?.sshTunnel?.host || "",
    port: profile?.sshTunnel?.port || 22,
    username: profile?.sshTunnel?.username || "",
    authMethod: profile?.sshTunnel?.authMethod || "Password",
    privateKeyPath: profile?.sshTunnel?.privateKeyPath || "",
    localPort: profile?.sshTunnel?.localPort || 0,
  });
  const [sshPassword, setSshPassword] = useState("");
  const [showSshPassword, setShowSshPassword] = useState(false);
  const [enableSshKeychain, setEnableSshKeychain] = useState(true);

  // Update form when profile changes
  useEffect(() => {
    setFormData({
      id: profile?.id || "",
      name: profile?.name || "",
      driver: driver,
      host: profile?.host || "localhost",
      port: profile?.port || getDefaultPort(driver),
      username: profile?.username || "",
      database: profile?.database || "",
      sslMode: profile?.sslMode || "Disable",
    });
    setSshMode(profile?.sshTunnel ? "ssh" : "off");
    setSshConfig({
      host: profile?.sshTunnel?.host || "",
      port: profile?.sshTunnel?.port || 22,
      username: profile?.sshTunnel?.username || "",
      authMethod: profile?.sshTunnel?.authMethod || "Password",
      privateKeyPath: profile?.sshTunnel?.privateKeyPath || "",
      localPort: profile?.sshTunnel?.localPort || 0,
    });
    setPassword("");
    setSshPassword("");
    setError(null);
    setTestStatus(null);
  }, [profile, driver]);

  // SSL modes
  const sslModes: SslMode[] = ["Disable", "Prefer", "Require"];

  // Handle form field changes
  const handleChange = (e: ChangeEvent<HTMLInputElement>) => {
    const { name, value } = e.target;
    if (name === "port") {
      setFormData((prev) => ({ ...prev, [name]: parseInt(value, 10) || 0 }));
    } else {
      setFormData((prev) => ({ ...prev, [name]: value }));
    }
  };

  // Handle SSH config changes
  const handleSshChange = (e: ChangeEvent<HTMLInputElement>) => {
    const { name, value } = e.target;
    if (name === "port" || name === "localPort") {
      setSshConfig((prev) => ({ ...prev, [name]: parseInt(value, 10) || 0 }));
    } else {
      setSshConfig((prev) => ({ ...prev, [name]: value }));
    }
  };

  // Parse connection string to auto-fill fields
  const handleConnectionStringChange = (value: string) => {
    setConnectionString(value);
    if (!value.trim()) return;

    try {
      // Basic connection string parsing
      const patterns: Record<string, RegExp> = {
        Postgres: /^postgres(?:ql)?:\/\/(?:([^:@]+)(?::([^@]*))?@)?([^:/?]+)(?::(\d+))?(?:\/(.+))?$/,
        MySql: /^mysql:\/\/(?:([^:@]+)(?::([^@]*))?@)?([^:/?]+)(?::(\d+))?(?:\/(.+))?$/,
        MongoDb: /^mongodb(?:\+srv)?:\/\/(?:([^:@]+)(?::([^@]*))?@)?([^:/?]+)(?::(\d+))?(?:\/(.+))?$/,
        SqlServer: /^(?:mssql|sqlserver):\/\/(?:([^:@]+)(?::([^@]*))?@)?([^:/?]+)(?::(\d+))?(?:\/(.+))?$/,
      };

      const pattern = patterns[driver];
      if (!pattern) return;

      const match = value.match(pattern);
      if (match) {
        const [, user, pass, host, port, database] = match;
        setFormData((prev) => ({
          ...prev,
          username: user || prev.username || "",
          host: host || prev.host || "localhost",
          port: port ? parseInt(port, 10) : prev.port,
          database: database || prev.database || "",
        }));
        if (pass) setPassword(pass);
      }
    } catch {
      // Ignore parse errors
    }
  };

  // File picker for SQLite
  const handleBrowseFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          { name: "SQLite Database", extensions: ["db", "sqlite", "sqlite3", "db3"] },
          { name: "All Files", extensions: ["*"] },
        ],
      });
      if (selected && typeof selected === "string") {
        setFormData((prev) => ({ ...prev, database: selected }));
      }
    } catch (err) {
      console.error("Failed to open file picker:", err);
    }
  };

  // File picker for SSH private key
  const handleBrowseKeyFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          { name: "SSH Private Key", extensions: ["pem", "key", ""] },
          { name: "All Files", extensions: ["*"] },
        ],
      });
      if (selected && typeof selected === "string") {
        setSshConfig((prev) => ({ ...prev, privateKeyPath: selected }));
      }
    } catch (err) {
      console.error("Failed to open file picker:", err);
    }
  };

  // Validate form
  const validateForm = (): string | null => {
    if (!formData.name?.trim()) return "Connection name is required";

    if (driver === "Sqlite") {
      if (!formData.database?.trim()) return "Database file path is required for SQLite";
      return null;
    }

    if (!formData.host?.trim()) return "Host is required";
    if (driver !== "MongoDb" && !formData.username?.trim()) return "Username is required";
    if (formData.port !== undefined && (formData.port < 0 || formData.port > 65535))
      return "Port must be between 0 and 65535";

    return null;
  };

  // Build SSH config from state
  const buildSshConfig = (): SshConfig | null => {
    if (sshMode === "off" || !sshConfig.host || !sshConfig.username) return null;
    return {
      host: sshConfig.host,
      port: sshConfig.port || 22,
      username: sshConfig.username,
      authMethod: (sshConfig.authMethod as SshAuthMethod) || "Password",
      privateKeyPath: sshConfig.privateKeyPath || null,
      keyPassphraseKeyringKey: null,
      localPort: sshConfig.localPort || 0,
    };
  };

  // Build profile from form state
  const buildProfile = (): ConnectionProfile => ({
    id: formData.id || "",
    name: formData.name!,
    driver: driver,
    host: formData.host!,
    port: formData.port!,
    username: formData.username || "",
    database: formData.database || null,
    sslMode: formData.sslMode!,
    passwordKeyringKey: null,
    sshTunnel: buildSshConfig(),
    folder: null,
    environment: null,
    lastConnectedAt: profile?.lastConnectedAt || null,
    connectionCount: profile?.connectionCount || 0,
    isFavorite: profile?.isFavorite || false,
    color: profile?.color || null,
    description: profile?.description || null,
    createdAt: profile?.createdAt || Math.floor(Date.now() / 1000),
    updatedAt: Math.floor(Date.now() / 1000),
  });

  // Test connection
  const handleTestConnection = async () => {
    setError(null);
    setTestStatus(null);

    const validationError = validateForm();
    if (validationError) {
      setError(validationError);
      return;
    }

    setLoading(true);

    try {
      const testProfile = buildProfile();
      const status = await invoke<ConnectionStatus>("test_connection_command", {
        profile: testProfile,
        password,
        sshPassword: sshMode === "ssh" && sshConfig.authMethod === "Password" ? sshPassword : null,
      });

      if (status === "Connected") {
        setTestStatus("Connection successful!");
        if (testProfile.id && enableKeychain) {
          try {
            await invoke("save_password", { profileId: testProfile.id, password });
            if (sshMode === "ssh" && sshConfig.authMethod === "Password" && sshPassword && enableSshKeychain) {
              await invoke("save_ssh_password", { profileId: testProfile.id, sshPassword });
            }
          } catch (err) {
            console.error("Failed to save password:", err);
          }
        }
      } else {
        setTestStatus("Connection failed");
      }
    } catch (err) {
      const errorMessage = typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Connection test failed: ${errorMessage}`);
    } finally {
      setLoading(false);
    }
  };

  // Save connection
  const handleSave = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setTestStatus(null);

    const validationError = validateForm();
    if (validationError) {
      setError(validationError);
      return;
    }

    setLoading(true);

    try {
      const saveProfile = buildProfile();
      let profileId: string;

      if (profile) {
        await invoke("update_connection_profile", { profile: saveProfile });
        profileId = saveProfile.id;
      } else {
        profileId = await invoke<string>("create_connection_profile", { profile: saveProfile });
      }

      if (password && enableKeychain) {
        try {
          await invoke("save_password", { profileId, password });
          if (sshMode === "ssh" && sshConfig.authMethod === "Password" && sshPassword && enableSshKeychain) {
            await invoke("save_ssh_password", { profileId, sshPassword });
          }
        } catch (err) {
          console.error("Failed to save password:", err);
        }
      }

      onSuccess?.(profileId);
    } catch (err) {
      const errorMessage = typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to save profile: ${errorMessage}`);
    } finally {
      setLoading(false);
    }
  };

  // Get placeholder for connection string
  const getConnectionStringPlaceholder = () => {
    switch (driver) {
      case "Postgres": return "postgresql://user:password@host:5432/database";
      case "MySql": return "mysql://user:password@host:3306/database";
      case "MongoDb": return "mongodb://user:password@host:27017/database";
      case "SqlServer": return "mssql://user:password@host:1433/database";
      default: return "";
    }
  };

  return (
    <form onSubmit={handleSave} className="space-y-6">
      {/* Connection Name */}
      <div className="space-y-2">
        <Label htmlFor="name" className="text-sm font-medium">
          Connection Name
        </Label>
        <Input
          type="text"
          id="name"
          name="name"
          value={formData.name || ""}
          onChange={handleChange}
          placeholder={`My ${getDriverDisplayName(driver)}`}
          required
          className="h-11"
        />
        <p className="text-xs text-muted-foreground">
          A friendly name to identify this connection
        </p>
      </div>

      {/* Tags placeholder */}
      <div className="space-y-2">
        <Label className="text-sm font-medium">Tags</Label>
        <button
          type="button"
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-md border border-dashed border-muted-foreground/30 text-sm text-muted-foreground hover:border-muted-foreground/50 transition-colors"
        >
          <span>+</span> Add tags
          <span className="text-xs">&#x25BE;</span>
        </button>
      </div>

      {/* SQLite: File path */}
      {driver === "Sqlite" ? (
        <div className="space-y-2">
          <Label htmlFor="database" className="text-sm font-medium">
            Database File
          </Label>
          <div className="flex gap-2">
            <Input
              type="text"
              id="database"
              name="database"
              value={formData.database || ""}
              onChange={handleChange}
              placeholder="/path/to/database.db"
              required
              className="flex-1 h-11"
            />
            <Button type="button" variant="outline" onClick={handleBrowseFile} className="h-11">
              Browse
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            Select or enter the path to your SQLite database file
          </p>
        </div>
      ) : (
        <>
          {/* Connection String (optional) */}
          <div className="space-y-2">
            <Label className="text-sm font-medium">
              Connection String <span className="text-muted-foreground font-normal">(optional)</span>
            </Label>
            <Input
              value={connectionString}
              onChange={(e) => handleConnectionStringChange(e.target.value)}
              placeholder={getConnectionStringPlaceholder()}
              className="h-11"
            />
            <p className="text-xs text-muted-foreground">
              Paste a connection string to auto-fill the fields below
            </p>
          </div>

          {/* Host and Port */}
          <div className="grid grid-cols-5 gap-4">
            <div className="col-span-3 space-y-2">
              <Label htmlFor="host" className="text-sm font-medium">Host</Label>
              <Input
                type="text"
                id="host"
                name="host"
                value={formData.host || ""}
                onChange={handleChange}
                placeholder="localhost"
                required
                className="h-11"
              />
            </div>
            <div className="col-span-2 space-y-2">
              <Label htmlFor="port" className="text-sm font-medium">Port</Label>
              <Input
                type="number"
                id="port"
                name="port"
                value={formData.port || 0}
                onChange={handleChange}
                min="0"
                max="65535"
                required
                className="h-11"
              />
            </div>
          </div>

          {/* Database Name */}
          <div className="space-y-2">
            <Label htmlFor="database" className="text-sm font-medium">
              Database Name <span className="text-muted-foreground font-normal">(optional)</span>
            </Label>
            <Input
              type="text"
              id="database"
              name="database"
              value={formData.database || ""}
              onChange={handleChange}
              placeholder="Leave empty to select database after connecting"
              className="h-11"
            />
          </div>

          {/* Username and Password */}
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="username" className="text-sm font-medium">
                Username
                {driver === "MongoDb" && (
                  <span className="text-muted-foreground font-normal ml-1">(optional)</span>
                )}
              </Label>
              <Input
                type="text"
                id="username"
                name="username"
                value={formData.username || ""}
                onChange={handleChange}
                placeholder={driver === "MongoDb" ? "root (optional)" : "postgres"}
                required={driver !== "MongoDb"}
                className="h-11"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="password" className="text-sm font-medium">Password</Label>
              <div className="relative">
                <Input
                  type={showPassword ? "text" : "password"}
                  id="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder="&#8226;&#8226;&#8226;&#8226;&#8226;&#8226;&#8226;&#8226;"
                  className="pr-10 h-11"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="absolute right-0 top-0 h-full px-3 hover:bg-transparent"
                  onClick={() => setShowPassword(!showPassword)}
                >
                  {showPassword ? (
                    <EyeOff className="h-4 w-4 text-muted-foreground" />
                  ) : (
                    <Eye className="h-4 w-4 text-muted-foreground" />
                  )}
                </Button>
              </div>
              <div className="flex items-center gap-2 mt-1.5">
                <Checkbox
                  id="enable-keychain"
                  checked={enableKeychain}
                  onCheckedChange={(checked) => setEnableKeychain(checked === true)}
                />
                <label htmlFor="enable-keychain" className="text-xs text-muted-foreground cursor-pointer flex items-center gap-1">
                  <Key className="h-3 w-3" />
                  Enable keychain
                </label>
              </div>
            </div>
          </div>

          {/* SSL Mode */}
          <div className="space-y-2">
            <Label className="text-sm font-medium">SSL Mode</Label>
            <Select
              value={formData.sslMode || "Disable"}
              onValueChange={(value) =>
                setFormData((prev) => ({ ...prev, sslMode: value as SslMode }))
              }
            >
              <SelectTrigger className="w-full h-11">
                <SelectValue placeholder="Select SSL mode" />
              </SelectTrigger>
              <SelectContent>
                {sslModes.map((mode) => (
                  <SelectItem key={mode} value={mode}>
                    {mode === "Disable" ? "Disabled" : mode === "Prefer" ? "Preferred" : "Required"}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* SSH Tunnel */}
          <div className="space-y-4">
            <Label className="text-sm font-medium">SSH Tunnel</Label>
            <div className="inline-flex rounded-lg border border-border overflow-hidden">
              <button
                type="button"
                onClick={() => setSshMode("off")}
                className={`px-5 py-2 text-sm font-medium transition-colors ${
                  sshMode === "off"
                    ? "bg-accent text-foreground"
                    : "bg-background text-muted-foreground hover:text-foreground"
                }`}
              >
                Off
              </button>
              <button
                type="button"
                onClick={() => setSshMode("ssh")}
                className={`px-5 py-2 text-sm font-medium transition-colors border-l border-border ${
                  sshMode === "ssh"
                    ? "bg-accent text-foreground"
                    : "bg-background text-muted-foreground hover:text-foreground"
                }`}
              >
                Over SSH
              </button>
            </div>

            {sshMode === "ssh" && (
              <div className="space-y-5 pt-2">
                {/* SSH Server and Port */}
                <div className="grid grid-cols-5 gap-4">
                  <div className="col-span-3 space-y-2">
                    <Label htmlFor="ssh-host" className="text-sm font-medium">SSH Server</Label>
                    <Input
                      type="text"
                      id="ssh-host"
                      name="host"
                      value={sshConfig.host || ""}
                      onChange={handleSshChange}
                      placeholder="192.168.1.1"
                      className="h-11"
                    />
                  </div>
                  <div className="col-span-2 space-y-2">
                    <Label htmlFor="ssh-port" className="text-sm font-medium">Port</Label>
                    <Input
                      type="number"
                      id="ssh-port"
                      name="port"
                      value={sshConfig.port || 22}
                      onChange={handleSshChange}
                      min="0"
                      max="65535"
                      className="h-11"
                    />
                  </div>
                </div>

                {/* SSH Username */}
                <div className="space-y-2">
                  <Label htmlFor="ssh-username" className="text-sm font-medium">SSH Username</Label>
                  <Input
                    type="text"
                    id="ssh-username"
                    name="username"
                    value={sshConfig.username || ""}
                    onChange={handleSshChange}
                    placeholder="ubuntu"
                    className="h-11"
                  />
                </div>

                {/* Authentication Method Toggle */}
                <div className="space-y-3">
                  <Label className="text-sm font-medium">Authentication</Label>
                  <div className="inline-flex rounded-lg border border-border overflow-hidden">
                    <button
                      type="button"
                      onClick={() =>
                        setSshConfig((prev) => ({ ...prev, authMethod: "Password" }))
                      }
                      className={`px-5 py-2 text-sm font-medium transition-colors ${
                        sshConfig.authMethod === "Password"
                          ? "bg-accent text-foreground"
                          : "bg-background text-muted-foreground hover:text-foreground"
                      }`}
                    >
                      Password
                    </button>
                    <button
                      type="button"
                      onClick={() =>
                        setSshConfig((prev) => ({ ...prev, authMethod: "PrivateKey" }))
                      }
                      className={`px-5 py-2 text-sm font-medium transition-colors border-l border-border flex items-center gap-1.5 ${
                        sshConfig.authMethod === "PrivateKey"
                          ? "bg-accent text-foreground"
                          : "bg-background text-muted-foreground hover:text-foreground"
                      }`}
                    >
                      <Key className="h-3.5 w-3.5" />
                      Private Key
                    </button>
                  </div>
                </div>

                {/* SSH Password or Private Key */}
                {sshConfig.authMethod === "Password" ? (
                  <div className="space-y-2">
                    <Label htmlFor="ssh-password" className="text-sm font-medium">SSH Password</Label>
                    <div className="relative">
                      <Input
                        type={showSshPassword ? "text" : "password"}
                        id="ssh-password"
                        value={sshPassword}
                        onChange={(e) => setSshPassword(e.target.value)}
                        placeholder="&#8226;&#8226;&#8226;&#8226;&#8226;&#8226;&#8226;&#8226;"
                        className="pr-10 h-11"
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="absolute right-0 top-0 h-full px-3 hover:bg-transparent"
                        onClick={() => setShowSshPassword(!showSshPassword)}
                      >
                        {showSshPassword ? (
                          <EyeOff className="h-4 w-4 text-muted-foreground" />
                        ) : (
                          <Eye className="h-4 w-4 text-muted-foreground" />
                        )}
                      </Button>
                    </div>
                    <div className="flex items-center gap-2 mt-1.5">
                      <Checkbox
                        id="enable-ssh-keychain"
                        checked={enableSshKeychain}
                        onCheckedChange={(checked) => setEnableSshKeychain(checked === true)}
                      />
                      <label htmlFor="enable-ssh-keychain" className="text-xs text-muted-foreground cursor-pointer flex items-center gap-1">
                        <Key className="h-3 w-3" />
                        Enable keychain
                      </label>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      Having trouble? Check SSH server requirements
                    </p>
                  </div>
                ) : (
                  <div className="space-y-2">
                    <Label htmlFor="ssh-key-path" className="text-sm font-medium">Private Key File</Label>
                    <div className="flex gap-2">
                      <Input
                        type="text"
                        id="ssh-key-path"
                        name="privateKeyPath"
                        value={sshConfig.privateKeyPath || ""}
                        onChange={handleSshChange}
                        placeholder="/path/to/id_rsa"
                        className="flex-1 h-11"
                      />
                      <Button type="button" variant="outline" onClick={handleBrowseKeyFile} className="h-11">
                        Browse
                      </Button>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      Path to your SSH private key file
                    </p>
                  </div>
                )}
              </div>
            )}
          </div>
        </>
      )}

      {/* Error Message */}
      {error && (
        <div className="rounded-lg bg-destructive/10 border border-destructive/20 p-3 text-sm text-destructive">
          {error}
        </div>
      )}

      {/* Success Message */}
      {testStatus && (
        <div className="rounded-lg bg-green-50 dark:bg-green-950/20 border border-green-200 dark:border-green-900 p-3 text-sm text-green-600 dark:text-green-400 flex items-center gap-2">
          <svg className="h-4 w-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
          </svg>
          {testStatus}
        </div>
      )}

      {/* Buttons */}
      <div className="flex gap-3 pt-4">
        <Button
          type="button"
          variant="outline"
          onClick={handleTestConnection}
          disabled={loading}
          className="flex-1 h-11"
        >
          {loading ? "Testing..." : "Test Connection"}
        </Button>
        <Button
          type="submit"
          disabled={loading}
          className="flex-1 h-11"
        >
          {loading ? "Saving..." : "Save Connection"}
        </Button>
      </div>
    </form>
  );
};
