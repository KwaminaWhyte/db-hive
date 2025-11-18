import { FC, useState, FormEvent, ChangeEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  ConnectionProfile,
  DbDriver,
  SslMode,
  ConnectionStatus,
  DbError,
  getDefaultPort,
  getDriverDisplayName,
} from '../types/database';

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
      } else {
        setTestStatus('Connection failed');
      }
    } catch (err) {
      const dbError = err as DbError;
      setError(`Connection test failed: ${dbError.message}`);
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
      const dbError = err as DbError;
      setError(`Failed to save profile: ${dbError.message}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div style={{ padding: '20px', border: '1px solid #ccc', borderRadius: '8px' }}>
      <h2>{profile ? 'Edit Connection' : 'New Connection'}</h2>

      <form onSubmit={handleSave}>
        {/* Connection Name */}
        <div style={{ marginBottom: '15px' }}>
          <label htmlFor="name" style={{ display: 'block', marginBottom: '5px' }}>
            Connection Name *
          </label>
          <input
            type="text"
            id="name"
            name="name"
            value={formData.name || ''}
            onChange={handleChange}
            style={{ width: '100%', padding: '8px', fontSize: '14px' }}
            placeholder="My Database"
            required
          />
        </div>

        {/* Database Driver */}
        <div style={{ marginBottom: '15px' }}>
          <label htmlFor="driver" style={{ display: 'block', marginBottom: '5px' }}>
            Database Driver *
          </label>
          <select
            id="driver"
            name="driver"
            value={formData.driver || 'Postgres'}
            onChange={handleChange}
            style={{ width: '100%', padding: '8px', fontSize: '14px' }}
            required
          >
            {drivers.map((driver) => (
              <option key={driver} value={driver}>
                {getDriverDisplayName(driver)}
              </option>
            ))}
          </select>
        </div>

        {/* Host */}
        <div style={{ marginBottom: '15px' }}>
          <label htmlFor="host" style={{ display: 'block', marginBottom: '5px' }}>
            Host *
          </label>
          <input
            type="text"
            id="host"
            name="host"
            value={formData.host || ''}
            onChange={handleChange}
            style={{ width: '100%', padding: '8px', fontSize: '14px' }}
            placeholder="localhost"
            required
          />
        </div>

        {/* Port */}
        <div style={{ marginBottom: '15px' }}>
          <label htmlFor="port" style={{ display: 'block', marginBottom: '5px' }}>
            Port *
          </label>
          <input
            type="number"
            id="port"
            name="port"
            value={formData.port || 0}
            onChange={handleChange}
            style={{ width: '100%', padding: '8px', fontSize: '14px' }}
            min="0"
            max="65535"
            required
          />
        </div>

        {/* Username */}
        <div style={{ marginBottom: '15px' }}>
          <label htmlFor="username" style={{ display: 'block', marginBottom: '5px' }}>
            Username *
          </label>
          <input
            type="text"
            id="username"
            name="username"
            value={formData.username || ''}
            onChange={handleChange}
            style={{ width: '100%', padding: '8px', fontSize: '14px' }}
            placeholder="postgres"
            required
          />
        </div>

        {/* Password */}
        <div style={{ marginBottom: '15px' }}>
          <label htmlFor="password" style={{ display: 'block', marginBottom: '5px' }}>
            Password
          </label>
          <input
            type="password"
            id="password"
            name="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            style={{ width: '100%', padding: '8px', fontSize: '14px' }}
            placeholder="••••••••"
          />
        </div>

        {/* Database */}
        <div style={{ marginBottom: '15px' }}>
          <label htmlFor="database" style={{ display: 'block', marginBottom: '5px' }}>
            Database (optional)
          </label>
          <input
            type="text"
            id="database"
            name="database"
            value={formData.database || ''}
            onChange={handleChange}
            style={{ width: '100%', padding: '8px', fontSize: '14px' }}
            placeholder="mydb"
          />
        </div>

        {/* SSL Mode */}
        <div style={{ marginBottom: '15px' }}>
          <label htmlFor="sslMode" style={{ display: 'block', marginBottom: '5px' }}>
            SSL Mode
          </label>
          <select
            id="sslMode"
            name="sslMode"
            value={formData.sslMode || 'Prefer'}
            onChange={handleChange}
            style={{ width: '100%', padding: '8px', fontSize: '14px' }}
          >
            {sslModes.map((mode) => (
              <option key={mode} value={mode}>
                {mode}
              </option>
            ))}
          </select>
        </div>

        {/* Error Message */}
        {error && (
          <div
            style={{
              marginBottom: '15px',
              padding: '10px',
              backgroundColor: '#fee',
              color: '#c33',
              borderRadius: '4px',
            }}
          >
            {error}
          </div>
        )}

        {/* Success Message */}
        {testStatus && (
          <div
            style={{
              marginBottom: '15px',
              padding: '10px',
              backgroundColor: '#efe',
              color: '#3a3',
              borderRadius: '4px',
            }}
          >
            {testStatus}
          </div>
        )}

        {/* Buttons */}
        <div style={{ display: 'flex', gap: '10px' }}>
          <button
            type="button"
            onClick={handleTestConnection}
            disabled={loading}
            style={{
              padding: '10px 20px',
              fontSize: '14px',
              cursor: loading ? 'not-allowed' : 'pointer',
              backgroundColor: '#4a90e2',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
            }}
          >
            {loading ? 'Testing...' : 'Test Connection'}
          </button>

          <button
            type="submit"
            disabled={loading}
            style={{
              padding: '10px 20px',
              fontSize: '14px',
              cursor: loading ? 'not-allowed' : 'pointer',
              backgroundColor: '#5cb85c',
              color: 'white',
              border: 'none',
              borderRadius: '4px',
            }}
          >
            {loading ? 'Saving...' : 'Save'}
          </button>
        </div>
      </form>
    </div>
  );
};
