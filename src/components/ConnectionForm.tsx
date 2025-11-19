import { FC, useState, FormEvent, ChangeEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  ConnectionProfile,
  DbDriver,
  SslMode,
  ConnectionStatus,
  getDefaultPort,
  getDriverDisplayName,
} from '../types/database';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card';

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
    id: profile?.id || '',
    name: profile?.name || '',
    driver: profile?.driver || 'Postgres',
    host: profile?.host || 'localhost',
    port: profile?.port || 5432,
    username: profile?.username || '',
    database: profile?.database || '',
    sslMode: profile?.sslMode || 'Prefer',
  });

  const [password, setPassword] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [testStatus, setTestStatus] = useState<string | null>(null);

  // Available database drivers
  const drivers: DbDriver[] = [
    'Postgres',
    'MySql',
    'Sqlite',
    'MongoDb',
    'SqlServer',
  ];

  // Available SSL modes
  const sslModes: SslMode[] = ['Disable', 'Prefer', 'Require'];

  // Handle form field changes
  const handleChange = (
    e: ChangeEvent<HTMLInputElement | HTMLSelectElement>
  ) => {
    const { name, value } = e.target;

    // Handle driver change - update port to default
    if (name === 'driver') {
      const newDriver = value as DbDriver;
      setFormData((prev) => ({
        ...prev,
        driver: newDriver,
        port: getDefaultPort(newDriver),
      }));
    } else if (name === 'port') {
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

  // Validate form
  const validateForm = (): string | null => {
    if (!formData.name?.trim()) {
      return 'Connection name is required';
    }
    if (!formData.driver) {
      return 'Database driver is required';
    }
    if (!formData.host?.trim()) {
      return 'Host is required';
    }
    if (!formData.username?.trim()) {
      return 'Username is required';
    }
    if (formData.port !== undefined && (formData.port < 0 || formData.port > 65535)) {
      return 'Port must be between 0 and 65535';
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

    if (!password) {
      setError('Password is required to test connection');
      return;
    }

    setLoading(true);

    try {
      const testProfile: ConnectionProfile = {
        id: formData.id || '',
        name: formData.name!,
        driver: formData.driver!,
        host: formData.host!,
        port: formData.port!,
        username: formData.username!,
        database: formData.database || null,
        sslMode: formData.sslMode!,
        passwordKeyringKey: null,
        sshTunnel: null,
        folder: null,
      };

      const status = await invoke<ConnectionStatus>('test_connection_command', {
        profile: testProfile,
        password,
      });

      if (status === 'Connected') {
        setTestStatus('Connection successful!');

        // Save password for future use (if profile has an ID)
        if (testProfile.id) {
          try {
            await invoke('save_password', {
              profileId: testProfile.id,
              password,
            });
          } catch (err) {
            console.error('Failed to save password:', err);
            // Don't show error to user, connection test was successful
          }
        }
      } else {
        setTestStatus('Connection failed');
      }
    } catch (err) {
      // Handle error - could be string or DbError object
      const errorMessage = typeof err === 'string'
        ? err
        : (err as any)?.message || String(err);
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
      const saveProfile: ConnectionProfile = {
        id: formData.id || '',
        name: formData.name!,
        driver: formData.driver!,
        host: formData.host!,
        port: formData.port!,
        username: formData.username!,
        database: formData.database || null,
        sslMode: formData.sslMode!,
        passwordKeyringKey: null,
        sshTunnel: null,
        folder: null,
      };

      const profileId = await invoke<string>('create_connection_profile', {
        profile: saveProfile,
      });

      // Save password if provided
      if (password) {
        try {
          await invoke('save_password', {
            profileId,
            password,
          });
        } catch (err) {
          console.error('Failed to save password:', err);
          // Don't show error to user, profile was saved successfully
        }
      }

      setTestStatus('Connection profile saved successfully!');
      onSuccess?.(profileId);

      // Reset form if creating new profile
      if (!profile) {
        setFormData({
          id: '',
          name: '',
          driver: 'Postgres',
          host: 'localhost',
          port: 5432,
          username: '',
          database: '',
          sslMode: 'Prefer',
        });
        setPassword('');
      }
    } catch (err) {
      // Handle error - could be string or DbError object
      const errorMessage = typeof err === 'string'
        ? err
        : (err as any)?.message || String(err);
      setError(`Failed to save profile: ${errorMessage}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <Card className="m-6">
      <CardHeader>
        <CardTitle>{profile ? 'Edit Connection' : 'New Connection'}</CardTitle>
        <CardDescription>
          {profile
            ? 'Update your database connection settings'
            : 'Configure a new database connection'}
        </CardDescription>
      </CardHeader>
      <CardContent>
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
              value={formData.name || ''}
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
              value={formData.driver || 'Postgres'}
              onValueChange={(value) =>
                handleChange({
                  target: { name: 'driver', value },
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
                value={formData.host || ''}
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
              Username <span className="text-destructive">*</span>
            </Label>
            <Input
              type="text"
              id="username"
              name="username"
              value={formData.username || ''}
              onChange={handleChange}
              placeholder="postgres"
              required
            />
          </div>

          {/* Password */}
          <div className="space-y-2">
            <Label htmlFor="password">Password</Label>
            <Input
              type="password"
              id="password"
              name="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="••••••••"
            />
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
              value={formData.database || ''}
              onChange={handleChange}
              placeholder="mydb"
            />
            <p className="text-xs text-muted-foreground">
              The specific database to connect to. Leave empty to connect to the default database.
              <br />
              <span className="font-medium text-yellow-600 dark:text-yellow-500">
                Note: After connecting, you can only view tables from this database. To switch databases, edit this field and reconnect.
              </span>
            </p>
          </div>

          {/* SSL Mode */}
          <div className="space-y-2">
            <Label htmlFor="sslMode">SSL Mode</Label>
            <Select
              value={formData.sslMode || 'Prefer'}
              onValueChange={(value) =>
                handleChange({
                  target: { name: 'sslMode', value },
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
              {loading ? 'Testing...' : 'Test Connection'}
            </Button>

            <Button type="submit" disabled={loading} className="flex-1">
              {loading ? 'Saving...' : 'Save'}
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  );
};
