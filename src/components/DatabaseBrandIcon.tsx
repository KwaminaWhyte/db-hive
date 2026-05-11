import { FC } from "react";
import {
  SiPostgresql,
  SiMysql,
  SiMariadb,
  SiSqlite,
  SiMongodb,
  SiSupabase,
  SiRedis,
  SiTurso,
} from "react-icons/si";
import { Database } from "lucide-react";
import type { DbDriver } from "@/types/database";

interface BrandSpec {
  Icon: FC<{ size?: number; color?: string; className?: string }>;
  color: string;
}

const BRANDS: Record<string, BrandSpec> = {
  Postgres: { Icon: SiPostgresql, color: "#336791" },
  postgres: { Icon: SiPostgresql, color: "#336791" },
  postgresql: { Icon: SiPostgresql, color: "#336791" },
  MySql: { Icon: SiMysql, color: "#00758F" },
  mysql: { Icon: SiMysql, color: "#00758F" },
  MariaDb: { Icon: SiMariadb, color: "#003545" },
  mariadb: { Icon: SiMariadb, color: "#003545" },
  Sqlite: { Icon: SiSqlite, color: "#003B57" },
  sqlite: { Icon: SiSqlite, color: "#003B57" },
  MongoDb: { Icon: SiMongodb, color: "#13AA52" },
  mongodb: { Icon: SiMongodb, color: "#13AA52" },
  Supabase: { Icon: SiSupabase, color: "#3ECF8E" },
  supabase: { Icon: SiSupabase, color: "#3ECF8E" },
  Redis: { Icon: SiRedis, color: "#DC382D" },
  redis: { Icon: SiRedis, color: "#DC382D" },
  Turso: { Icon: SiTurso, color: "#4FF8D2" },
  turso: { Icon: SiTurso, color: "#4FF8D2" },
};

const FALLBACK_COLORS: Record<string, string> = {
  SqlServer: "#CC2927",
  sqlserver: "#CC2927",
  Neon: "#00A88E",
  neon: "#00A88E",
};

interface DatabaseBrandIconProps {
  driver: DbDriver | string;
  size?: number;
  className?: string;
}

export const DatabaseBrandIcon: FC<DatabaseBrandIconProps> = ({
  driver,
  size = 24,
  className,
}) => {
  const brand = BRANDS[driver];
  if (brand) {
    const { Icon, color } = brand;
    return <Icon size={size} color={color} className={className} />;
  }
  const fallbackColor = FALLBACK_COLORS[driver] ?? "#6b7280";
  return (
    <Database
      size={size}
      color={fallbackColor}
      className={className}
      strokeWidth={1.75}
    />
  );
};
