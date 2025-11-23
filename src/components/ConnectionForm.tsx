import { FC, useState, useEffect, FormEvent, ChangeEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Eye,
  EyeOff,
  ChevronDown,
  ChevronRight,
  Check,
  ChevronsUpDown,
  FolderOpen,
} from "lucide-react";
import {
  ConnectionProfile,
  DbDriver,
  SslMode,
  SshConfig,
  SshAuthMethod,
  ConnectionStatus,
  Environment,
  getDefaultPort,
  getDriverDisplayName,
} from "../types/database";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

interface ConnectionFormProps {
  /** Existing profile to edit, or undefined for new profile */
  profile?: ConnectionProfile;
  /** Callback when profile is successfully saved */
  onSuccess?: (profileId: string) => void;
}

export const ConnectionForm: FC<ConnectionFormProps> = ({
  profile,
  onSuccess,
}) => {
  // Form state
  const [formData, setFormData] = useState<Partial<ConnectionProfile>>({
    id: profile?.id || "",
    name: profile?.name || "",
    driver: profile?.driver || "Postgres",
    host: profile?.host || "localhost",
    port: profile?.port || 5432,
    username: profile?.username || "",
    database: profile?.database || "",
    sslMode: profile?.sslMode || "Prefer",
    folder: profile?.folder || null,
    environment: profile?.environment || null,
  });

  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [testStatus, setTestStatus] = useState<string | null>(null);

  // Folder management state
  const [existingFolders, setExistingFolders] = useState<string[]>([]);
  const [folderOpen, setFolderOpen] = useState(false);
  const [folderSearchValue, setFolderSearchValue] = useState("");

  // SSH Tunnel state
  const [sshEnabled, setSshEnabled] = useState(!!profile?.sshTunnel);
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

  // Load existing folders on mount
  useEffect(() => {
    const loadFolders = async () => {
      try {
        const profiles = await invoke<ConnectionProfile[]>(
          "list_connection_profiles"
        );
        const folders = profiles
          .filter((p) => p.folder)
          .map((p) => p.folder!)
          .filter((value, index, self) => self.indexOf(value) === index) // unique
          .sort();
        setExistingFolders(folders);
      } catch (error) {
        console.error("Failed to load folders:", error);
      }
    };
    loadFolders();
  }, []);

  // Update form data when profile prop changes
  useEffect(() => {
    setFormData({
      id: profile?.id || "",
      name: profile?.name || "",
      driver: profile?.driver || "Postgres",
      host: profile?.host || "localhost",
      port: profile?.port || 5432,
      username: profile?.username || "",
      database: profile?.database || "",
      sslMode: profile?.sslMode || "Prefer",
      folder: profile?.folder || null,
      environment: profile?.environment || null,
    });

    // Update SSH state
    setSshEnabled(!!profile?.sshTunnel);
    setSshConfig({
      host: profile?.sshTunnel?.host || "",
      port: profile?.sshTunnel?.port || 22,
      username: profile?.sshTunnel?.username || "",
      authMethod: profile?.sshTunnel?.authMethod || "Password",
      privateKeyPath: profile?.sshTunnel?.privateKeyPath || "",
      localPort: profile?.sshTunnel?.localPort || 0,
    });

    // Clear passwords and status when switching profiles
    setPassword("");
    setSshPassword("");
    setError(null);
    setTestStatus(null);
  }, [profile]);

  // Available database drivers
  const drivers: DbDriver[] = [
    "Postgres",
    "MySql",
    "Sqlite",
    "MongoDb",
    "SqlServer",
  ];

  // Available SSL modes
  const sslModes: SslMode[] = ["Disable", "Prefer", "Require"];

  // Handle form field changes
  const handleChange = (
    e: ChangeEvent<HTMLInputElement | HTMLSelectElement>
  ) => {
    const { name, value } = e.target;

    // Handle driver change - update port to default
    if (name === "driver") {
      const newDriver = value as DbDriver;
      setFormData((prev) => ({
        ...prev,
        driver: newDriver,
        port: getDefaultPort(newDriver),
      }));
    } else if (name === "port") {
      // Convert port to number
      setFormData((prev) => ({
        ...prev,
        [name]: parseInt(value, 10) || 0,
      }));
    } else {
      setFormData((prev) => ({
        ...prev,
        [name]: value,
      }));
    }
  };

  // Handle SSH config changes
  const handleSshChange = (
    e: ChangeEvent<HTMLInputElement | HTMLSelectElement>
  ) => {
    const { name, value } = e.target;

    if (name === "port" || name === "localPort") {
      setSshConfig((prev) => ({
        ...prev,
        [name]: parseInt(value, 10) || 0,
      }));
    } else {
      setSshConfig((prev) => ({
        ...prev,
        [name]: value,
      }));
    }
  };

  // Handle file picker for SQLite
  const handleBrowseFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: "SQLite Database",
            extensions: ["db", "sqlite", "sqlite3", "db3"],
          },
          {
            name: "All Files",
            extensions: ["*"],
          },
        ],
      });

      if (selected && typeof selected === "string") {
        setFormData((prev) => ({
          ...prev,
          database: selected,
        }));
      }
    } catch (err) {
      console.error("Failed to open file picker:", err);
      setError("Failed to open file picker");
    }
  };

  // Handle file picker for SSH private key
  const handleBrowseKeyFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: "SSH Private Key",
            extensions: ["pem", "key", ""],
          },
          {
            name: "All Files",
            extensions: ["*"],
          },
        ],
      });

      if (selected && typeof selected === "string") {
        setSshConfig((prev) => ({
          ...prev,
          privateKeyPath: selected,
        }));
      }
    } catch (err) {
      console.error("Failed to open file picker:", err);
      setError("Failed to open SSH key file picker");
    }
  };

  // Validate form
  const validateForm = (): string | null => {
    if (!formData.name?.trim()) {
      return "Connection name is required";
    }
    if (!formData.driver) {
      return "Database driver is required";
    }

    // SQLite validation
    if (formData.driver === "Sqlite") {
      if (!formData.database?.trim()) {
        return "Database file path is required for SQLite";
      }
      return null;
    }

    // Server-based database validation
    if (!formData.host?.trim()) {
      return "Host is required";
    }

    // MongoDB allows optional username/password for local connections
    // PostgreSQL, MySQL, and SQL Server require username
    if (formData.driver !== "MongoDb" && !formData.username?.trim()) {
      return "Username is required";
    }

    if (
      formData.port !== undefined &&
      (formData.port < 0 || formData.port > 65535)
    ) {
      return "Port must be between 0 and 65535";
    }
    return null;
  };

  // Test connection
  const handleTestConnection = async () => {
    setError(null);
    setTestStatus(null);

    const validationError = validateForm();
    if (validationError) {
      setError(validationError);
      return;
    }

    // SQLite and local MongoDB don't require password
    // Only require password for PostgreSQL and MySQL
    if (
      (formData.driver === "Postgres" || formData.driver === "MySql") &&
      !password
    ) {
      setError("Password is required for this database type");
      return;
    }

    setLoading(true);

    try {
      // Build SSH config if enabled
      const sshTunnel: SshConfig | null =
        sshEnabled && sshConfig.host && sshConfig.username
          ? {
              host: sshConfig.host,
              port: sshConfig.port || 22,
              username: sshConfig.username,
              authMethod: (sshConfig.authMethod as SshAuthMethod) || "Password",
              privateKeyPath: sshConfig.privateKeyPath || null,
              keyPassphraseKeyringKey: null, // TODO: Implement key passphrase storage
              localPort: sshConfig.localPort || 0,
            }
          : null;

      const testProfile: ConnectionProfile = {
        id: formData.id || "",
        name: formData.name!,
        driver: formData.driver!,
        host: formData.host!,
        port: formData.port!,
        username: formData.username || "",
        database: formData.database || null,
        sslMode: formData.sslMode!,
        passwordKeyringKey: null,
        sshTunnel,
        folder: null,
        environment: null,
        lastConnectedAt: null,
        connectionCount: 0,
        isFavorite: false,
        color: null,
        description: null,
        createdAt: Math.floor(Date.now() / 1000),
        updatedAt: Math.floor(Date.now() / 1000),
      };

      const status = await invoke<ConnectionStatus>("test_connection_command", {
        profile: testProfile,
        password,
        sshPassword:
          sshEnabled && sshConfig.authMethod === "Password"
            ? sshPassword
            : null,
      });

      if (status === "Connected") {
        setTestStatus("Connection successful!");

        // Save password for future use (if profile has an ID)
        if (testProfile.id) {
          try {
            await invoke("save_password", {
              profileId: testProfile.id,
              password,
            });

            // Save SSH password if enabled and using password auth
            if (
              sshEnabled &&
              sshConfig.authMethod === "Password" &&
              sshPassword
            ) {
              await invoke("save_ssh_password", {
                profileId: testProfile.id,
                sshPassword,
              });
            }
          } catch (err) {
            console.error("Failed to save password:", err);
            // Don't show error to user, connection test was successful
          }
        }
      } else {
        setTestStatus("Connection failed");
      }
    } catch (err) {
      // Handle error - could be string or DbError object
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Connection test failed: ${errorMessage}`);
    } finally {
      setLoading(false);
    }
  };

  // Save connection profile
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
      // Build SSH config if enabled
      const sshTunnel: SshConfig | null =
        sshEnabled && sshConfig.host && sshConfig.username
          ? {
              host: sshConfig.host,
              port: sshConfig.port || 22,
              username: sshConfig.username,
              authMethod: (sshConfig.authMethod as SshAuthMethod) || "Password",
              privateKeyPath: sshConfig.privateKeyPath || null,
              keyPassphraseKeyringKey: null, // TODO: Implement key passphrase storage
              localPort: sshConfig.localPort || 0,
            }
          : null;

      const saveProfile: ConnectionProfile = {
        id: formData.id || "",
        name: formData.name!,
        driver: formData.driver!,
        host: formData.host!,
        port: formData.port!,
        username: formData.username || "",
        database: formData.database || null,
        sslMode: formData.sslMode!,
        passwordKeyringKey: null,
        sshTunnel,
        folder: formData.folder || null,
        environment: formData.environment || null,

        // Metadata fields - preserve existing values when editing, use defaults for new profiles
        lastConnectedAt: profile?.lastConnectedAt || null,
        connectionCount: profile?.connectionCount || 0,
        isFavorite: profile?.isFavorite || false,
        color: profile?.color || null,
        description: profile?.description || null,
        createdAt: profile?.createdAt || Math.floor(Date.now() / 1000),
        updatedAt: Math.floor(Date.now() / 1000),
      };

      let profileId: string;

      if (profile) {
        // Editing existing profile - use update command
        await invoke("update_connection_profile", {
          profile: saveProfile,
        });
        profileId = saveProfile.id;
      } else {
        // Creating new profile
        profileId = await invoke<string>("create_connection_profile", {
          profile: saveProfile,
        });
      }

      // Save password if provided
      if (password) {
        try {
          await invoke("save_password", {
            profileId,
            password,
          });

          // Save SSH password if enabled and using password auth
          if (
            sshEnabled &&
            sshConfig.authMethod === "Password" &&
            sshPassword
          ) {
            await invoke("save_ssh_password", {
              profileId,
              sshPassword,
            });
          }
        } catch (err) {
          console.error("Failed to save password:", err);
          // Don't show error to user, profile was saved successfully
        }
      }

      setTestStatus("Connection profile saved successfully!");
      onSuccess?.(profileId);

      // Reset form if creating new profile
      if (!profile) {
        setFormData({
          id: "",
          name: "",
          driver: "Postgres",
          host: "localhost",
          port: 5432,
          username: "",
          database: "",
          sslMode: "Prefer",
        });
        setPassword("");
      }
    } catch (err) {
      // Handle error - could be string or DbError object
      const errorMessage =
        typeof err === "string" ? err : (err as any)?.message || String(err);
      setError(`Failed to save profile: ${errorMessage}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <form onSubmit={handleSave} className="space-y-4">
      {/* Connection Name */}
      <div className="space-y-2">
        <Label htmlFor="name">
          Connection Name <span className="text-destructive">*</span>
        </Label>
        <Input
          type="text"
          id="name"
          name="name"
          value={formData.name || ""}
          onChange={handleChange}
          placeholder="My Database"
          required
        />
      </div>

      {/* Database Driver */}
      <div className="space-y-2">
        <Label htmlFor="driver">
          Database Driver <span className="text-destructive">*</span>
        </Label>
        <Select
          value={formData.driver || "Postgres"}
          onValueChange={(value) =>
            handleChange({
              target: { name: "driver", value },
            } as ChangeEvent<HTMLSelectElement>)
          }
        >
          <SelectTrigger className="w-full">
            <SelectValue placeholder="Select database type" />
          </SelectTrigger>
          <SelectContent>
            {drivers.map((driver) => (
              <SelectItem key={driver} value={driver}>
                {getDriverDisplayName(driver)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Organization: Folder and Environment - Side by side */}
      <div className="grid grid-cols-2 gap-4">
        {/* Folder */}
        <div className="space-y-2">
          <Label htmlFor="folder">Folder</Label>
          <Popover open={folderOpen} onOpenChange={setFolderOpen}>
            <PopoverTrigger asChild>
              <Button
                variant="outline"
                role="combobox"
                aria-expanded={folderOpen}
                className="w-full justify-between"
              >
                {formData.folder ? (
                  <span className="flex items-center gap-2">
                    <FolderOpen className="h-4 w-4" />
                    {formData.folder}
                  </span>
                ) : (
                  <span className="text-muted-foreground">
                    Select or create folder...
                  </span>
                )}
                <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
              </Button>
            </PopoverTrigger>
            <PopoverContent className="w-[200px] p-0">
              <div className="p-2">
                <Input
                  placeholder="Search or create folder..."
                  value={folderSearchValue}
                  onChange={(e) => setFolderSearchValue(e.target.value)}
                  className="mb-2"
                />
                {/* None option */}
                <Button
                  variant="ghost"
                  className="w-full justify-start text-sm"
                  onClick={() => {
                    setFormData({ ...formData, folder: null });
                    setFolderOpen(false);
                    setFolderSearchValue("");
                  }}
                >
                  <Check
                    className={`mr-2 h-4 w-4 ${
                      !formData.folder ? "opacity-100" : "opacity-0"
                    }`}
                  />
                  None
                </Button>
                {/* Existing folders filtered by search */}
                {existingFolders
                  .filter((folder) =>
                    folder
                      .toLowerCase()
                      .includes(folderSearchValue.toLowerCase())
                  )
                  .map((folder) => (
                    <Button
                      key={folder}
                      variant="ghost"
                      className="w-full justify-start text-sm"
                      onClick={() => {
                        setFormData({ ...formData, folder });
                        setFolderOpen(false);
                        setFolderSearchValue("");
                      }}
                    >
                      <Check
                        className={`mr-2 h-4 w-4 ${
                          formData.folder === folder
                            ? "opacity-100"
                            : "opacity-0"
                        }`}
                      />
                      <FolderOpen className="mr-2 h-4 w-4" />
                      {folder}
                    </Button>
                  ))}
                {/* Create new folder option */}
                {folderSearchValue &&
                  !existingFolders.some(
                    (f) => f.toLowerCase() === folderSearchValue.toLowerCase()
                  ) && (
                    <Button
                      variant="ghost"
                      className="w-full justify-start text-sm border-t mt-2 pt-2"
                      onClick={() => {
                        setFormData({ ...formData, folder: folderSearchValue });
                        setExistingFolders(
                          [...existingFolders, folderSearchValue].sort()
                        );
                        setFolderOpen(false);
                        setFolderSearchValue("");
                      }}
                    >
                      <Check className="mr-2 h-4 w-4 opacity-0" />
                      Create "{folderSearchValue}"
                    </Button>
                  )}
              </div>
            </PopoverContent>
          </Popover>
          <p className="text-xs text-muted-foreground">
            Group connections in folders
          </p>
        </div>

        {/* Environment */}
        <div className="space-y-2">
          <Label htmlFor="environment">Environment</Label>
          <Select
            value={formData.environment || "none"}
            onValueChange={(value) =>
              setFormData({
                ...formData,
                environment: value === "none" ? null : (value as Environment),
              })
            }
          >
            <SelectTrigger className="w-full">
              <SelectValue placeholder="Select environment..." />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">None</SelectItem>
              <SelectItem value="Local">Local</SelectItem>
              <SelectItem value="Staging">Staging</SelectItem>
              <SelectItem value="Production">Production</SelectItem>
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            Tag connection environment
          </p>
        </div>
      </div>

      {/* SQLite File Path */}
      {formData.driver === "Sqlite" ? (
        <div className="space-y-2">
          <Label htmlFor="database">
            Database File <span className="text-destructive">*</span>
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
              className="flex-1"
            />
            <Button type="button" variant="outline" onClick={handleBrowseFile}>
              Browse
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            Select or enter the path to your SQLite database file
          </p>
        </div>
      ) : (
        <>
          {/* Host and Port - Side by side */}
          <div className="grid grid-cols-3 gap-4">
            <div className="col-span-2 space-y-2">
              <Label htmlFor="host">
                Host <span className="text-destructive">*</span>
              </Label>
              <Input
                type="text"
                id="host"
                name="host"
                value={formData.host || ""}
                onChange={handleChange}
                placeholder="localhost"
                required
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="port">
                Port <span className="text-destructive">*</span>
              </Label>
              <Input
                type="number"
                id="port"
                name="port"
                value={formData.port || 0}
                onChange={handleChange}
                min="0"
                max="65535"
                required
              />
            </div>
          </div>

          {/* Username */}
          <div className="space-y-2">
            <Label htmlFor="username">
              Username{" "}
              {formData.driver !== "MongoDb" && (
                <span className="text-destructive">*</span>
              )}
              {formData.driver === "MongoDb" && (
                <span className="text-muted-foreground">(optional)</span>
              )}
            </Label>
            <Input
              type="text"
              id="username"
              name="username"
              value={formData.username || ""}
              onChange={handleChange}
              placeholder={
                formData.driver === "MongoDb" ? "root (optional)" : "postgres"
              }
              required={formData.driver !== "MongoDb"}
            />
          </div>

          {/* Password */}
          <div className="space-y-2">
            <Label htmlFor="password">Password</Label>
            <div className="relative">
              <Input
                type={showPassword ? "text" : "password"}
                id="password"
                name="password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="••••••••"
                className="pr-10"
              />
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="absolute right-0 top-0 h-full px-3 py-2 hover:bg-transparent"
                onClick={() => setShowPassword(!showPassword)}
              >
                {showPassword ? (
                  <EyeOff className="h-4 w-4 text-muted-foreground" />
                ) : (
                  <Eye className="h-4 w-4 text-muted-foreground" />
                )}
              </Button>
            </div>
          </div>

          {/* Database */}
          <div className="space-y-2">
            <Label htmlFor="database">
              Database <span className="text-muted-foreground">(optional)</span>
            </Label>
            <Input
              type="text"
              id="database"
              name="database"
              value={formData.database || ""}
              onChange={handleChange}
              placeholder="mydb"
            />
            <p className="text-xs text-muted-foreground">
              The specific database to connect to. Leave empty to connect to the
              default database.
            </p>
          </div>

          {/* SSL Mode */}
          <div className="space-y-2">
            <Label htmlFor="sslMode">SSL Mode</Label>
            <Select
              value={formData.sslMode || "Prefer"}
              onValueChange={(value) =>
                handleChange({
                  target: { name: "sslMode", value },
                } as ChangeEvent<HTMLSelectElement>)
              }
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder="Select SSL mode" />
              </SelectTrigger>
              <SelectContent>
                {sslModes.map((mode) => (
                  <SelectItem key={mode} value={mode}>
                    {mode}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* SSH Tunnel Configuration */}
          <div className="space-y-3 pt-2">
            <div
              className="flex items-center gap-2 cursor-pointer select-none"
              onClick={() => setSshEnabled(!sshEnabled)}
            >
              {sshEnabled ? (
                <ChevronDown className="h-4 w-4 text-muted-foreground" />
              ) : (
                <ChevronRight className="h-4 w-4 text-muted-foreground" />
              )}
              <Label className="cursor-pointer">
                SSH Tunnel{" "}
                <span className="text-muted-foreground">(optional)</span>
              </Label>
            </div>

            {sshEnabled && (
              <div className="space-y-4 pl-6 pt-2 border-l-2 border-border">
                {/* SSH Host and Port */}
                <div className="grid grid-cols-3 gap-4">
                  <div className="col-span-2 space-y-2">
                    <Label htmlFor="ssh-host">
                      SSH Host <span className="text-destructive">*</span>
                    </Label>
                    <Input
                      type="text"
                      id="ssh-host"
                      name="host"
                      value={sshConfig.host || ""}
                      onChange={handleSshChange}
                      placeholder="ssh.example.com"
                      required={sshEnabled}
                    />
                  </div>

                  <div className="space-y-2">
                    <Label htmlFor="ssh-port">
                      SSH Port <span className="text-destructive">*</span>
                    </Label>
                    <Input
                      type="number"
                      id="ssh-port"
                      name="port"
                      value={sshConfig.port || 22}
                      onChange={handleSshChange}
                      min="0"
                      max="65535"
                      required={sshEnabled}
                    />
                  </div>
                </div>

                {/* SSH Username */}
                <div className="space-y-2">
                  <Label htmlFor="ssh-username">
                    SSH Username <span className="text-destructive">*</span>
                  </Label>
                  <Input
                    type="text"
                    id="ssh-username"
                    name="username"
                    value={sshConfig.username || ""}
                    onChange={handleSshChange}
                    placeholder="ubuntu"
                    required={sshEnabled}
                  />
                </div>

                {/* SSH Auth Method */}
                <div className="space-y-2">
                  <Label htmlFor="ssh-auth-method">Authentication Method</Label>
                  <Select
                    value={sshConfig.authMethod || "Password"}
                    onValueChange={(value) =>
                      handleSshChange({
                        target: { name: "authMethod", value },
                      } as ChangeEvent<HTMLSelectElement>)
                    }
                  >
                    <SelectTrigger className="w-full">
                      <SelectValue placeholder="Select auth method" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="Password">Password</SelectItem>
                      <SelectItem value="PrivateKey">Private Key</SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                {/* SSH Password or Private Key */}
                {sshConfig.authMethod === "Password" ? (
                  <div className="space-y-2">
                    <Label htmlFor="ssh-password">SSH Password</Label>
                    <div className="relative">
                      <Input
                        type={showSshPassword ? "text" : "password"}
                        id="ssh-password"
                        name="ssh-password"
                        value={sshPassword}
                        onChange={(e) => setSshPassword(e.target.value)}
                        placeholder="••••••••"
                        className="pr-10"
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="absolute right-0 top-0 h-full px-3 py-2 hover:bg-transparent"
                        onClick={() => setShowSshPassword(!showSshPassword)}
                      >
                        {showSshPassword ? (
                          <EyeOff className="h-4 w-4 text-muted-foreground" />
                        ) : (
                          <Eye className="h-4 w-4 text-muted-foreground" />
                        )}
                      </Button>
                    </div>
                  </div>
                ) : (
                  <div className="space-y-2">
                    <Label htmlFor="ssh-key-path">
                      Private Key File{" "}
                      <span className="text-destructive">*</span>
                    </Label>
                    <div className="flex gap-2">
                      <Input
                        type="text"
                        id="ssh-key-path"
                        name="privateKeyPath"
                        value={sshConfig.privateKeyPath || ""}
                        onChange={handleSshChange}
                        placeholder="/path/to/id_rsa"
                        required={
                          sshEnabled && sshConfig.authMethod === "PrivateKey"
                        }
                        className="flex-1"
                      />
                      <Button
                        type="button"
                        variant="outline"
                        onClick={handleBrowseKeyFile}
                      >
                        Browse
                      </Button>
                    </div>
                    <p className="text-xs text-muted-foreground">
                      Path to your SSH private key file
                    </p>
                  </div>
                )}

                {/* Local Port (Optional) */}
                <div className="space-y-2">
                  <Label htmlFor="local-port">
                    Local Port{" "}
                    <span className="text-muted-foreground">
                      (0 = auto-assign)
                    </span>
                  </Label>
                  <Input
                    type="number"
                    id="local-port"
                    name="localPort"
                    value={sshConfig.localPort || 0}
                    onChange={handleSshChange}
                    min="0"
                    max="65535"
                    placeholder="0"
                  />
                  <p className="text-xs text-muted-foreground">
                    Leave as 0 to automatically assign a free port
                  </p>
                </div>
              </div>
            )}
          </div>
        </>
      )}

      {/* Error Message */}
      {error && (
        <div className="rounded-md bg-destructive/10 p-3 text-sm text-destructive border border-destructive/20">
          {error}
        </div>
      )}

      {/* Success Message */}
      {testStatus && (
        <div className="rounded-md bg-green-50 dark:bg-green-950/20 p-3 text-sm text-green-600 dark:text-green-400 border border-green-200 dark:border-green-900">
          {testStatus}
        </div>
      )}

      {/* Buttons */}
      <div className="flex gap-3 pt-2">
        <Button
          type="button"
          variant="outline"
          onClick={handleTestConnection}
          disabled={loading}
          className="flex-1"
        >
          {loading ? "Testing..." : "Test Connection"}
        </Button>

        <Button type="submit" disabled={loading} className="flex-1">
          {loading ? "Saving..." : "Save"}
        </Button>
      </div>
    </form>
  );
};
